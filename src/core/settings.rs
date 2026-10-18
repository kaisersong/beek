//! Windows settings contract. Credentials are immutable references; the full
//! provider snapshot and its reference commit together, then the old secret retires.
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Provider {
    pub credential_ref: Option<String>,
    pub model: String,
    pub base_url: String,
    // Read legacy plaintext once for migration; never emit it in new files.
    #[serde(skip_serializing)]
    pub api_key: String,
}
impl Default for Provider {
    fn default() -> Self {
        Self {
            credential_ref: None,
            model: super::prompts::DEFAULT_MODEL.into(),
            base_url: super::prompts::DEFAULT_ENDPOINT.into(),
            api_key: String::new(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub schema: u32,
    pub selection_enabled: bool,
    pub copy_to_popup: bool,
    pub autostart: bool,
    pub hotkey_enabled: bool,
    pub hotkey: String,
    pub provider: Provider,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: 1,
            selection_enabled: true,
            copy_to_popup: false,
            autostart: false,
            hotkey_enabled: true,
            hotkey: "Ctrl+Alt+T".into(),
            provider: Provider::default(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 {
            return Err("不支持的配置版本，请使用相应版本 BeeK".into());
        }
        if self.provider.base_url.len() > 4096
            || self.hotkey.len() > 64
            || self
                .provider
                .credential_ref
                .as_ref()
                .is_some_and(|r| r.is_empty() || r.len() > 128)
            || self.provider.api_key.len() > 2560
        {
            return Err("配置字段超出长度限制，原配置已保留".into());
        }
        if self.provider.model.trim().is_empty() || self.provider.model.len() > 1024 {
            return Err("模型名称无效".into());
        }
        let url = reqwest::Url::parse(&self.provider.base_url).map_err(|_| "API 地址无效")?;
        if url.scheme() != "https"
            && !(url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")))
        {
            return Err("API 地址需为 HTTPS（本机测试可用 HTTP）".into());
        }
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err("API 地址不能包含账号、密码或片段".into());
        }
        Ok(())
    }
}
pub enum KeyEdit {
    Keep,
    Replace(String),
    Clear,
}
pub trait Backend {
    fn create_secret(&mut self, value: &str) -> Result<String, String>;
    fn read_secret(&self, reference: &str) -> Result<String, String>;
    fn delete_secret(&mut self, reference: &str) -> Result<(), String>;
    fn commit(&mut self, settings: &Settings) -> Result<(), String>;
}
pub fn transact(
    store: &mut impl Backend,
    old: &Settings,
    mut next: Settings,
    edit: KeyEdit,
) -> Result<Settings, String> {
    next.validate()?;
    let edit = match edit {
        KeyEdit::Keep
            if old.provider.credential_ref.is_none() && !old.provider.api_key.is_empty() =>
        {
            KeyEdit::Replace(old.provider.api_key.clone())
        }
        edit => edit,
    };
    let staged = match edit {
        KeyEdit::Keep => {
            next.provider.credential_ref = old.provider.credential_ref.clone();
            None
        }
        KeyEdit::Clear => {
            next.provider.credential_ref = None;
            None
        }
        KeyEdit::Replace(value) => {
            let value = value.trim();
            if value.is_empty() || value.len() > 2560 {
                return Err("API Key 为空或过长".into());
            }
            let reference = store.create_secret(value)?;
            match store.read_secret(&reference) {
                Ok(actual) if actual == value => {}
                _ => {
                    let _ = store.delete_secret(&reference);
                    return Err("凭据写入后复核失败，原配置已保留".into());
                }
            }
            next.provider.credential_ref = Some(reference.clone());
            Some(reference)
        }
    };
    next.provider.api_key.clear();
    if let Err(e) = store.commit(&next) {
        if let Some(r) = staged {
            let _ = store.delete_secret(&r);
        }
        return Err(e);
    }
    if old.provider.credential_ref != next.provider.credential_ref {
        if let Some(r) = &old.provider.credential_ref {
            let _ = store.delete_secret(r);
        }
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    #[derive(Default)]
    struct Memory {
        secrets: HashMap<String, String>,
        disk: Option<Settings>,
        fail: bool,
    }
    impl Backend for Memory {
        fn create_secret(&mut self, v: &str) -> Result<String, String> {
            let r = format!("new-{}", self.secrets.len());
            self.secrets.insert(r.clone(), v.into());
            Ok(r)
        }
        fn read_secret(&self, r: &str) -> Result<String, String> {
            self.secrets.get(r).cloned().ok_or("missing".into())
        }
        fn delete_secret(&mut self, r: &str) -> Result<(), String> {
            self.secrets.remove(r);
            Ok(())
        }
        fn commit(&mut self, s: &Settings) -> Result<(), String> {
            if self.fail {
                Err("disk full".into())
            } else {
                self.disk = Some(s.clone());
                Ok(())
            }
        }
    }
    #[test]
    fn failed_config_commit_preserves_old_key_and_provider() {
        let mut m = Memory::default();
        let old = transact(
            &mut m,
            &Settings::default(),
            Settings::default(),
            KeyEdit::Replace("old-key".into()),
        )
        .unwrap();
        let oldref = old.provider.credential_ref.clone().unwrap();
        m.fail = true;
        let mut next = old.clone();
        next.provider.model = "new-model".into();
        assert!(transact(&mut m, &old, next, KeyEdit::Replace("new-key".into())).is_err());
        assert_eq!(m.read_secret(&oldref).unwrap(), "old-key");
        assert_eq!(m.disk.unwrap().provider.model, old.provider.model);
        assert_eq!(m.secrets.len(), 1);
    }
    #[test]
    fn successful_commit_never_serializes_key_and_clear_is_explicit() {
        let mut m = Memory::default();
        let s = transact(
            &mut m,
            &Settings::default(),
            Settings::default(),
            KeyEdit::Replace("private-key".into()),
        )
        .unwrap();
        assert!(!serde_json::to_string(&s).unwrap().contains("private-key"));
        let r = s.provider.credential_ref.clone().unwrap();
        let kept = transact(&mut m, &s, s.clone(), KeyEdit::Keep).unwrap();
        assert_eq!(kept.provider.credential_ref, s.provider.credential_ref);
        let clear = transact(&mut m, &kept, kept.clone(), KeyEdit::Clear).unwrap();
        assert!(clear.provider.credential_ref.is_none());
        assert!(m.read_secret(&r).is_err());
    }
    #[test]
    fn missing_fields_do_not_enable_clipboard_or_autostart() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(!s.copy_to_popup && !s.autostart);
        s.validate().unwrap();
    }
    #[test]
    fn keeping_a_legacy_key_migrates_transactionally_instead_of_erasing_it() {
        let mut old = Settings::default();
        old.provider.api_key = "public-legacy-fixture".into();
        let mut memory = Memory {
            fail: true,
            ..Default::default()
        };
        assert!(transact(&mut memory, &old, old.clone(), KeyEdit::Keep).is_err());
        assert_eq!(old.provider.api_key, "public-legacy-fixture");
        assert!(memory.secrets.is_empty());
        memory.fail = false;
        let saved = transact(&mut memory, &old, old.clone(), KeyEdit::Keep).unwrap();
        assert_eq!(
            memory
                .read_secret(saved.provider.credential_ref.as_ref().unwrap())
                .unwrap(),
            "public-legacy-fixture"
        );
        assert!(!serde_json::to_string(&saved)
            .unwrap()
            .contains("public-legacy-fixture"));
    }
    #[test]
    fn accepted_configuration_fields_are_bounded_for_reload() {
        let mut s = Settings::default();
        s.provider.base_url = format!("https://example.invalid/{}", "x".repeat(4096));
        assert!(s.validate().is_err());
        s.provider.base_url = Provider::default().base_url;
        s.hotkey = "x".repeat(65);
        assert!(s.validate().is_err());
    }
}
