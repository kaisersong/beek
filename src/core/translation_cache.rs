//! Process-local translation results. No disk writes or background cleanup task.
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const MAX_ENTRIES: usize = 1000;
pub const MAX_TTL_MINUTES: u64 = 24 * 60;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_RESULT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CacheSettings {
    pub max_entries: usize,
    pub ttl_minutes: u64,
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            max_entries: 128,
            ttl_minutes: 30,
        }
    }
}

impl<'de> Deserialize<'de> for CacheSettings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let defaults = Self::default();
        // Tolerate invalid new fields without losing the rest of an older config.
        fn bounded(v: Option<&serde_json::Value>, default: u64, max: u64) -> u64 {
            let Some(v) = v else { return default };
            if let Some(n) = v.as_u64() {
                return n.min(max);
            }
            if v.as_f64().is_some_and(|n| n < 0.) {
                return 0;
            }
            if v.as_f64().is_some_and(|n| n >= max as f64) {
                return max;
            }
            default
        }
        Ok(Self {
            max_entries: bounded(
                value.get("max_entries"),
                defaults.max_entries as u64,
                MAX_ENTRIES as u64,
            ) as usize,
            ttl_minutes: bounded(
                value.get("ttl_minutes"),
                defaults.ttl_minutes,
                MAX_TTL_MINUTES,
            ),
        })
    }
}

impl CacheSettings {
    pub fn normalized(self) -> Self {
        Self {
            max_entries: self.max_entries.min(MAX_ENTRIES),
            ttl_minutes: self.ttl_minutes.min(MAX_TTL_MINUTES),
        }
    }

    pub fn enabled(self) -> bool {
        self.max_entries > 0 && self.ttl_minutes > 0
    }

    pub fn from_input(entries: &str, minutes: &str) -> Result<Self, String> {
        let max_entries = entries
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|n| *n <= MAX_ENTRIES)
            .ok_or_else(|| format!("缓存条数请输入 0–{} 的整数（0 表示关闭）。", MAX_ENTRIES))?;
        let ttl_minutes = minutes
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|n| *n <= MAX_TTL_MINUTES)
            .ok_or_else(|| {
                format!(
                    "有效时间请输入 0–{} 分钟的整数（0 表示关闭）。",
                    MAX_TTL_MINUTES
                )
            })?;
        Ok(Self {
            max_entries,
            ttl_minutes,
        })
    }
}

struct Entry {
    key: [u8; 32],
    result: String,
    created: Instant,
}

#[derive(Default)]
pub struct TranslationCache {
    entries: VecDeque<Entry>,
    bytes: usize,
    settings: CacheSettings,
}

impl TranslationCache {
    fn expire(&mut self, now: Instant) {
        let ttl = Duration::from_secs(self.settings.ttl_minutes * 60);
        self.entries
            .retain(|e| self.settings.enabled() && now.saturating_duration_since(e.created) < ttl);
        self.bytes = self.entries.iter().map(|e| e.result.len() + 32).sum();
    }

    pub fn apply_settings(&mut self, settings: CacheSettings, now: Instant) {
        self.settings = settings.normalized();
        self.expire(now);
        while self.entries.len() > self.settings.max_entries {
            let old = self.entries.pop_front().unwrap();
            self.bytes -= old.result.len() + 32;
        }
    }

    pub fn get(&mut self, key: &[u8; 32], now: Instant) -> Option<String> {
        self.expire(now);
        let index = self.entries.iter().position(|e| &e.key == key)?;
        let entry = self.entries.remove(index)?;
        let result = entry.result.clone();
        self.entries.push_back(entry);
        Some(result)
    }

    /// Call only after the stream has explicitly completed successfully.
    pub fn insert(&mut self, key: [u8; 32], result: String, now: Instant) {
        self.expire(now);
        if !self.settings.enabled() || result.trim().is_empty() || result.len() > MAX_RESULT_BYTES {
            return;
        }
        if let Some(index) = self.entries.iter().position(|e| e.key == key) {
            let old = self.entries.remove(index).unwrap();
            self.bytes -= old.result.len() + 32;
        }
        while self.entries.len() >= self.settings.max_entries
            || self.bytes + result.len() + 32 > MAX_BYTES
        {
            let Some(old) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= old.result.len() + 32;
        }
        self.bytes += result.len() + 32;
        self.entries.push_back(Entry {
            key,
            result,
            created: now,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TTL: Duration = Duration::from_secs(30 * 60);

    #[test]
    fn settings_are_bounded_and_bad_json_values_do_not_break_configuration() {
        assert_eq!(CacheSettings::default().max_entries, 128);
        assert_eq!(CacheSettings::default().ttl_minutes, 30);
        let settings: CacheSettings =
            serde_json::from_str(r#"{"max_entries":999999,"ttl_minutes":1e100}"#).unwrap();
        assert_eq!(settings.max_entries, 1000);
        assert_eq!(settings.ttl_minutes, 1440);
        let disabled: CacheSettings =
            serde_json::from_str(r#"{"max_entries":-2,"ttl_minutes":0}"#).unwrap();
        assert!(!disabled.enabled());
        let invalid: CacheSettings =
            serde_json::from_str(r#"{"max_entries":"bad","ttl_minutes":2.5}"#).unwrap();
        assert_eq!(invalid, CacheSettings::default());
        assert!(CacheSettings::from_input("1001", "30").is_err());
        assert!(CacheSettings::from_input("128", "1441").is_err());
        assert!(CacheSettings::from_input("1.5", "30").is_err());
        assert!(!CacheSettings::from_input("0", "30").unwrap().enabled());
    }

    #[test]
    fn applying_smaller_limits_or_disabling_immediately_removes_cached_results() {
        let mut cache = TranslationCache::default();
        let now = Instant::now();
        for i in 0..3 {
            cache.insert([i; 32], "result".into(), now);
        }
        cache.apply_settings(CacheSettings::from_input("1", "30").unwrap(), now);
        assert!(cache.get(&[0; 32], now).is_none());
        assert!(cache.get(&[2; 32], now).is_some());
        cache.apply_settings(
            CacheSettings::from_input("1", "1").unwrap(),
            now + Duration::from_secs(60),
        );
        assert!(cache.get(&[2; 32], now + Duration::from_secs(60)).is_none());
        cache.apply_settings(
            CacheSettings::from_input("0", "30").unwrap(),
            now + Duration::from_secs(60),
        );
        cache.insert(
            [3; 32],
            "late completed result".into(),
            now + Duration::from_secs(60),
        );
        assert!(cache.get(&[3; 32], now + Duration::from_secs(60)).is_none());
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn hits_do_not_extend_ttl_and_expiry_removes_results() {
        let mut cache = TranslationCache::default();
        let now = Instant::now();
        cache.insert([1; 32], "译文".into(), now);
        assert_eq!(cache.get(&[1; 32], now + TTL / 2).as_deref(), Some("译文"));
        assert!(cache.get(&[1; 32], now + TTL).is_none());
        assert_eq!(cache.bytes, 0);
    }

    #[test]
    fn lru_evicts_oldest_and_respects_count_and_byte_limits() {
        let mut cache = TranslationCache::default();
        let now = Instant::now();
        for i in 0..128 {
            cache.insert([i; 32], "result".into(), now);
        }
        cache.get(&[0; 32], now).unwrap();
        cache.insert([128; 32], "new".into(), now);
        assert!(cache.get(&[1; 32], now).is_none());
        assert!(cache.get(&[0; 32], now).is_some());
        for i in 0..128 {
            cache.insert([i; 32], "a".repeat(MAX_RESULT_BYTES), now);
        }
        assert!(cache.bytes <= MAX_BYTES);
        assert!(cache.entries.len() <= MAX_ENTRIES);
        let count = cache.entries.len();
        cache.insert([255; 32], "a".repeat(MAX_RESULT_BYTES + 1), now);
        cache.insert([254; 32], "  ".into(), now);
        assert_eq!(cache.entries.len(), count);
    }
}
