//! Windows settings contract. Credentials are immutable references; the full
//! provider snapshot and its reference commit together, then the old secret retires.
use super::{
    localization::UiLanguage,
    translation_cache::CacheSettings,
    translation_engine::{Engine, EngineConfig},
    translation_language::TargetLanguage,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Provider {
    pub credential_ref: Option<String>,
    pub model: String,
    pub base_url: String,
    pub app_id: String,
    pub region: String,
    pub source_language: String,
    pub email: String,
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
            app_id: String::new(),
            region: String::new(),
            source_language: "en".into(),
            email: String::new(),
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
    pub auto_update: bool,
    pub hotkey_enabled: bool,
    pub hotkey: String,
    pub provider: Provider,
    #[serde(default = "legacy_engine")]
    pub translation_engine: Engine,
    pub providers: BTreeMap<String, Provider>,
    pub translation_target: TargetLanguage,
    pub ui_language: UiLanguage,
    pub translation_cache: CacheSettings,
    pub onboarding_revision: u16,
}
fn legacy_engine() -> Engine {
    Engine::Ark
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: 1,
            selection_enabled: true,
            copy_to_popup: false,
            autostart: false,
            auto_update: true,
            hotkey_enabled: true,
            hotkey: "Ctrl+Alt+T".into(),
            provider: Provider::default(),
            translation_engine: Engine::PublicBaidu,
            providers: BTreeMap::new(),
            translation_target: TargetLanguage::default(),
            ui_language: UiLanguage::default(),
            translation_cache: CacheSettings::default(),
            onboarding_revision: 0,
        }
    }
}
impl Settings {
    pub fn provider_for(&self, engine: Engine) -> Provider {
        if engine == Engine::Ark {
            self.provider.clone()
        } else {
            self.providers
                .get(engine.key())
                .cloned()
                .unwrap_or_else(|| {
                    let mut p = Provider::default();
                    if engine == Engine::LibreTranslate {
                        p.base_url = "http://127.0.0.1:5000/translate".into();
                    }
                    p
                })
        }
    }
    pub fn active_provider(&self) -> Provider {
        self.provider_for(self.translation_engine)
    }
    pub fn active_provider_mut(&mut self) -> &mut Provider {
        if self.translation_engine == Engine::Ark {
            &mut self.provider
        } else {
            let provider = self.provider_for(self.translation_engine);
            self.providers
                .entry(self.translation_engine.key().into())
                .or_insert(provider)
        }
    }
    pub fn credential_refs(&self) -> BTreeSet<String> {
        std::iter::once(&self.provider)
            .chain(self.providers.values())
            .filter_map(|p| p.credential_ref.clone())
            .collect()
    }
    pub fn engine_config(&self, key: String) -> EngineConfig {
        let p = self.active_provider();
        EngineConfig {
            api_key: key,
            target_language: self.translation_target,
            model: Some(p.model),
            base_url: Some(p.base_url),
            app_id: p.app_id,
            region: p.region,
            source_language: p.source_language,
            email: p.email,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 {
            return Err("不支持的配置版本，请使用相应版本 BeeK".into());
        }
        if self.providers.len() > 7
            || self
                .providers
                .keys()
                .any(|k| !Engine::ALL.iter().any(|e| e.key() == k))
        {
            return Err("翻译服务配置无效".into());
        }
        for provider in std::iter::once(&self.provider).chain(self.providers.values()) {
            if provider.base_url.len() > 4096
                || self.hotkey.len() > 64
                || provider
                    .credential_ref
                    .as_ref()
                    .is_some_and(|r| r.is_empty() || r.len() > 128)
                || provider.api_key.len() > 2560
                || [
                    &provider.app_id,
                    &provider.region,
                    &provider.source_language,
                    &provider.email,
                ]
                .iter()
                .any(|s| s.len() > 4096)
            {
                return Err("配置字段超出长度限制，原配置已保留".into());
            }
            if provider.model.trim().is_empty() || provider.model.len() > 1024 {
                return Err("模型名称无效".into());
            }
            let url = reqwest::Url::parse(&provider.base_url).map_err(|_| "API 地址无效")?;
            if url.scheme() != "https"
                && !(url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")))
            {
                return Err("API 地址需为 HTTPS（本机测试可用 HTTP）".into());
            }
            if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
                return Err("API 地址不能包含账号、密码或片段".into());
            }
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
    let previous = old.provider_for(next.translation_engine);
    // References come only from the committed snapshot, never from form drafts.
    next.provider.credential_ref = old.provider.credential_ref.clone();
    for (name, provider) in &mut next.providers {
        provider.credential_ref = old
            .providers
            .get(name)
            .and_then(|p| p.credential_ref.clone());
        provider.api_key.clear();
    }
    let edit = match edit {
        KeyEdit::Keep if previous.credential_ref.is_none() && !previous.api_key.is_empty() => {
            KeyEdit::Replace(previous.api_key.clone())
        }
        edit => edit,
    };
    let staged = match edit {
        KeyEdit::Keep => {
            next.active_provider_mut().credential_ref = previous.credential_ref;
            None
        }
        KeyEdit::Clear => {
            next.active_provider_mut().credential_ref = None;
            None
        }
        KeyEdit::Replace(value) => {
            let value = value.trim();
            if value.is_empty() || value.len() > 2560 {
                return Err("API Key 为空或过长".into());
            }
            let reference = store.create_secret(value)?;
            if !store
                .read_secret(&reference)
                .is_ok_and(|actual| actual == value)
            {
                let _ = store.delete_secret(&reference);
                return Err("凭据写入后复核失败，原配置已保留".into());
            }
            next.active_provider_mut().credential_ref = Some(reference.clone());
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
    for reference in old.credential_refs().difference(&next.credential_refs()) {
        let _ = store.delete_secret(reference);
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    fn ark() -> Settings {
        Settings {
            translation_engine: Engine::Ark,
            ..Settings::default()
        }
    }
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
    fn switching_services_retains_both_keys_and_failed_replacement_rolls_back() {
        let mut m = Memory::default();
        let ark = transact(
            &mut m,
            &ark(),
            ark(),
            KeyEdit::Replace("ark-fixture".into()),
        )
        .unwrap();
        let mut azure = ark.clone();
        azure.translation_engine = Engine::Azure;
        let azure = transact(
            &mut m,
            &ark,
            azure,
            KeyEdit::Replace("azure-fixture".into()),
        )
        .unwrap();
        assert_eq!(azure.credential_refs().len(), 2);
        assert_eq!(m.secrets.len(), 2);
        let mut restored = azure.clone();
        restored.translation_engine = Engine::Ark;
        let restored = transact(&mut m, &azure, restored, KeyEdit::Keep).unwrap();
        assert_eq!(
            m.read_secret(restored.provider.credential_ref.as_ref().unwrap())
                .unwrap(),
            "ark-fixture"
        );
        m.fail = true;
        assert!(transact(
            &mut m,
            &restored,
            restored.clone(),
            KeyEdit::Replace("replacement".into())
        )
        .is_err());
        assert_eq!(m.secrets.len(), 2);
        m.fail = false;
        let cleared = transact(&mut m, &restored, restored.clone(), KeyEdit::Clear).unwrap();
        assert_eq!(cleared.credential_refs().len(), 1);
        assert_eq!(
            m.read_secret(
                cleared
                    .provider_for(Engine::Azure)
                    .credential_ref
                    .as_ref()
                    .unwrap()
            )
            .unwrap(),
            "azure-fixture"
        );
    }
    #[test]
    fn failed_config_commit_preserves_old_key_and_provider() {
        let mut m = Memory::default();
        let old = transact(&mut m, &ark(), ark(), KeyEdit::Replace("old-key".into())).unwrap();
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
            &ark(),
            ark(),
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
        let mut old = ark();
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
        let mut s = ark();
        s.provider.base_url = format!("https://example.invalid/{}", "x".repeat(4096));
        assert!(s.validate().is_err());
        s.provider.base_url = Provider::default().base_url;
        s.hotkey = "x".repeat(65);
        assert!(s.validate().is_err());
    }
}

#[cfg(test)]
mod parity_tests {
    use super::*;
    #[test]
    fn legacy_ark_is_preserved_and_new_users_start_without_a_key() {
        let cfg: Settings = serde_json::from_str(
            r#"{"provider":{"api_key":"public-legacy-fixture","model":"legacy-model"}}"#,
        )
        .unwrap();
        assert_eq!(cfg.translation_engine, Engine::Ark);
        assert_eq!(cfg.active_provider().model, "legacy-model");
        assert_eq!(Settings::default().translation_engine, Engine::PublicBaidu);
        let mut cfg = Settings::default();
        cfg.translation_engine = Engine::LibreTranslate;
        assert_eq!(
            cfg.active_provider().base_url,
            "http://127.0.0.1:5000/translate"
        );
    }
    #[test]
    fn saved_settings_preserve_languages_and_each_service_without_plaintext_keys() {
        let mut cfg = Settings::default();
        cfg.translation_engine = Engine::Azure;
        cfg.translation_target = TargetLanguage::Japanese;
        cfg.ui_language = UiLanguage::English;
        cfg.active_provider_mut().credential_ref = Some("azure-fixture-ref".into());
        cfg.active_provider_mut().region = "eastasia".into();
        cfg.translation_engine = Engine::Ark;
        cfg.active_provider_mut().credential_ref = Some("ark-fixture-ref".into());
        let json = serde_json::to_string(&cfg).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.translation_target, TargetLanguage::Japanese);
        assert_eq!(restored.ui_language, UiLanguage::English);
        assert_eq!(restored.provider_for(Engine::Azure).region, "eastasia");
        assert_eq!(restored.credential_refs().len(), 2);
    }
}
