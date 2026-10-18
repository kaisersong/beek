use super::protocol::Source;
use std::collections::{HashMap, VecDeque};

pub const QUERY_MS: u64 = 1500;
pub const PENDING_MS: u64 = 300;
pub const TTL_MS: u64 = 8000;

#[derive(Default)]
struct Fault {
    failures: u8,
    next: u64,
    quarantined: bool,
    probe_used: bool,
}
#[derive(Default)]
pub struct Health {
    sources: HashMap<(u32, u64), Fault>,
    attempts: VecDeque<u64>,
    infrastructure: u8,
    cross_source: Vec<(u32, u64)>,
    paused: bool,
}
impl Health {
    pub fn allowed(&self, source: Source, now: u64) -> bool {
        !self.paused
            && self
                .sources
                .get(&(source.pid, source.created))
                .is_none_or(|f| {
                    !f.quarantined && now >= f.next && !(f.failures >= 3 && f.probe_used)
                })
    }
    pub fn begin(&mut self, source: Source, now: u64) -> bool {
        if !self.allowed(source, now) {
            return false;
        }
        if let Some(f) = self.sources.get_mut(&(source.pid, source.created)) {
            if f.failures >= 3 {
                f.probe_used = true;
            }
        }
        true
    }
    /// Administrative cancellation is not a provider failure and cannot consume
    /// the sole recovery probe forever. Keep at most one probe in flight.
    pub fn abandon(&mut self, source: Source, now: u64) {
        if let Some(f) = self.sources.get_mut(&(source.pid, source.created)) {
            if f.failures == 3 && !f.quarantined && f.probe_used {
                f.probe_used = false;
                f.next = now.saturating_add(1000);
            }
        }
    }
    pub fn attempt(&mut self, now: u64) -> bool {
        while self
            .attempts
            .front()
            .is_some_and(|t| now.saturating_sub(*t) >= 10_000)
        {
            self.attempts.pop_front();
        }
        if self.attempts.len() >= 6 {
            return false;
        }
        self.attempts.push_back(now);
        true
    }
    pub fn success(&mut self, source: Source) {
        self.sources.remove(&(source.pid, source.created));
        self.infrastructure = 0;
        self.cross_source.clear();
    }
    pub fn failure(&mut self, source: Option<Source>, now: u64, protocol_fault: bool) {
        if let Some(s) = source {
            let key = (s.pid, s.created);
            let f = self.sources.entry(key).or_default();
            f.failures = f.failures.saturating_add(1);
            match f.failures {
                1 => f.next = now + 1000,
                2 => f.next = now + 5000,
                3 => f.next = now + 30_000,
                _ => f.quarantined = true,
            }
            if protocol_fault && !self.cross_source.contains(&key) {
                self.cross_source.push(key);
                if self.cross_source.len() >= 3 {
                    self.paused = true;
                }
            }
        } else {
            self.infrastructure = self.infrastructure.saturating_add(1);
            if self.infrastructure >= 3 {
                self.paused = true;
            }
        }
    }
    pub fn restore(&mut self) {
        self.sources.clear();
        self.infrastructure = 0;
        self.cross_source.clear();
        self.paused = false;
    }
    pub fn retain_live(&mut self, mut alive: impl FnMut(u32, u64) -> bool) {
        self.sources
            .retain(|(pid, created), _| alive(*pid, *created));
        self.cross_source
            .retain(|(pid, created)| alive(*pid, *created));
    }
    pub fn summary(&self, now: u64) -> (usize, usize, usize) {
        (
            self.sources.len(),
            self.sources.values().filter(|f| f.quarantined).count(),
            self.attempts
                .iter()
                .filter(|t| now.saturating_sub(**t) < 10000)
                .count(),
        )
    }
    pub fn paused(&self) -> bool {
        self.paused
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn src(pid: u32) -> Source {
        Source {
            hwnd: pid as u64,
            pid,
            created: 10,
            generation: 1,
        }
    }
    #[test]
    fn one_hung_application_never_disables_other_applications() {
        let mut h = Health::default();
        let s = src(1);
        for (now, wait) in [(0, 1000), (1000, 5000), (6000, 30_000)] {
            h.failure(Some(s), now, false);
            assert!(!h.allowed(s, now + wait - 1));
            assert!(h.allowed(s, now + wait));
            assert!(h.allowed(src(2), now));
        }
        h.failure(Some(s), 36_000, false);
        assert!(!h.allowed(s, 1_000_000));
        assert!(h.allowed(Source { created: 11, ..s }, 36_000));
        assert!(!h.paused());
    }
    #[test]
    fn spawn_budget_is_shared_and_counts_failed_attempts() {
        let mut h = Health::default();
        for _ in 0..6 {
            assert!(h.attempt(0));
        }
        assert!(!h.attempt(9999));
        h.restore();
        assert!(!h.attempt(9999));
        assert!(h.attempt(10_000));
    }
    #[test]
    fn three_distinct_protocol_faults_pause_platform_but_not_repeated_one_source() {
        let mut h = Health::default();
        for _ in 0..4 {
            h.failure(Some(src(1)), 0, true);
        }
        assert!(!h.paused());
        h.failure(Some(src(2)), 0, true);
        h.failure(Some(src(3)), 0, true);
        assert!(h.paused());
        h.restore();
        assert!(h.allowed(src(1), 0));
    }
    #[test]
    fn exactly_one_probe_and_restore_cannot_bypass_spawn_budget() {
        let mut h = Health::default();
        let s = src(7);
        for now in [0, 1000, 6000] {
            h.failure(Some(s), now, false);
        }
        assert!(!h.begin(s, 35999));
        assert!(h.begin(s, 36000));
        assert!(!h.begin(s, 36001));
        h.failure(Some(s), 36002, false);
        assert!(!h.begin(s, u64::MAX));
        for _ in 0..6 {
            assert!(h.attempt(36002));
        }
        h.restore();
        assert!(h.allowed(s, 36002));
        assert!(!h.attempt(36003));
    }
    #[test]
    fn infrastructure_pause_still_allows_bounded_explicit_clipboard() {
        let mut h = Health::default();
        for _ in 0..3 {
            h.failure(None, 0, true);
        }
        assert!(h.paused());
        assert!(!h.begin(src(1), 0));
        assert!(h.attempt(0));
        h.restore();
        assert!(h.begin(src(1), 0));
    }
    #[test]
    fn canceled_probe_can_recover_but_failed_probe_stays_quarantined() {
        let mut h = Health::default();
        let s = src(9);
        for now in [0, 1000, 6000] {
            h.failure(Some(s), now, false);
        }
        assert!(h.begin(s, 36000));
        assert!(!h.begin(s, 36001));
        h.abandon(s, 36010);
        assert!(!h.begin(s, 37009));
        assert!(h.begin(s, 37010));
        assert!(!h.begin(s, 37011));
        h.failure(Some(s), 37012, false);
        h.abandon(s, 37013);
        assert!(!h.begin(s, 1_000_000));
    }
}
