// BeeK 配置模块 — 持久化到 ~/Library/Application Support/BeeK/config.json
//
// 界面偏好与服务参数写入 JSON；Key 默认保留文件保存，可在服务设置中选择钥匙串。
// 只有显式保存对应服务才迁移 Key；钥匙串写入/读回失败保留原文件。
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStorage {
    #[default]
    File,
    Keychain,
}

/// 服务名常量：配置文件里的 key、日志里的名字都用它，避免各处写字符串字面量
pub const ARK: &str = "ark";

pub use beek::core::translation_engine::{Engine, EngineConfig as ProviderConfig};
pub use beek::core::translation_language::TargetLanguage;
pub use beek::core::localization::UiLanguage;

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(from = "ConfigWire")]
pub struct Config {
    /// Opaque immutable accounts for providers explicitly saved to Keychain.
    #[serde(default)]
    pub credential_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub credential_storage: BTreeMap<String, CredentialStorage>,
    pub translation_target: TargetLanguage,
    pub ui_language: UiLanguage,
    /// 是否在 Dock 显示图标，默认 false（只显示菜单栏图标）
    #[serde(default)]
    pub show_in_dock: bool,
    /// AX 取不到选区时，是否用「模拟 ⌘C + 读剪贴板」兜底。
    /// 默认开：Safari / Muse / Grok Bot 这类应用根本不把选区暴露给 AX，不开就完全取不到词。
    /// 复制前会先存原剪贴板，读完立刻还原。
    #[serde(default = "default_true")]
    pub clipboard_fallback: bool,
    /// 「复制即弹按钮」：你自己按 ⌘C 复制时，在鼠标旁弹小蜜蜂。
    /// **默认关**：这条会在每一次复制时弹按钮 —— 对经常复制的人等于持续打扰（实测反馈）。
    /// 它只对「AX 看不到选区」的应用（Safari / Muse / Grok Bot）才有额外价值，而那些应用
    /// 已经有「剪贴板兜底取词」（手势触发，不乱弹）。需要的人可以在菜单里手动打开。
    #[serde(default)]
    pub copy_to_popup: bool,
    /// 是否自动检查并下载更新（默认开）。下载完成后不会打断你：
    /// 退出 BeeK 时静默装上，或下次启动时生效。对应菜单里的「自动检查更新」。
    #[serde(default = "default_true")]
    pub auto_update: bool,
    /// Process-local result cache; bounded independently of user-edited JSON.
    #[serde(default)]
    pub translation_cache: beek::core::translation_cache::CacheSettings,
    /// 各翻译服务的配置，键是服务名（见 ARK）
    #[serde(default)]
    pub translation_engine: Engine,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub onboarding_revision: u16,
}

// Optional engine distinguishes legacy Ark accounts from a first launch.
#[derive(Deserialize)]
struct ConfigWire {
    #[serde(default)] credential_refs: BTreeMap<String, String>,
    #[serde(default)] credential_storage: BTreeMap<String, CredentialStorage>,
    #[serde(default)] translation_target: TargetLanguage,
    #[serde(default)] ui_language: UiLanguage,
    #[serde(default)] show_in_dock: bool,
    #[serde(default = "default_true")] clipboard_fallback: bool,
    #[serde(default)] copy_to_popup: bool,
    #[serde(default = "default_true")] auto_update: bool,
    #[serde(default)] translation_cache: beek::core::translation_cache::CacheSettings,
    #[serde(default)] translation_engine: Option<Engine>,
    #[serde(default)] providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)] onboarding_revision: u16,
}
impl From<ConfigWire> for Config {
    fn from(w: ConfigWire) -> Self {
        let translation_engine = w.translation_engine.unwrap_or_else(|| {
            if w.providers.contains_key(ARK) { Engine::Ark }
            else { Engine::PublicBaidu }
        });
        Self { credential_storage:w.credential_storage, credential_refs:w.credential_refs, translation_target:w.translation_target, ui_language:w.ui_language, show_in_dock:w.show_in_dock, clipboard_fallback:w.clipboard_fallback,
            copy_to_popup:w.copy_to_popup, auto_update:w.auto_update,
            translation_cache:w.translation_cache,translation_engine,providers:w.providers,onboarding_revision:w.onboarding_revision }
    }
}
impl Default for Config {
    fn default() -> Self {
        Self {credential_storage:Default::default(),credential_refs:Default::default(),translation_target:TargetLanguage::default(),ui_language:UiLanguage::default(),show_in_dock:false,clipboard_fallback:true,copy_to_popup:false,auto_update:true,
            translation_cache:Default::default(),translation_engine:Engine::PublicBaidu,providers:Default::default(),onboarding_revision:0}
    }
}

impl Config {
    #[cfg(test)]
    pub fn provider(&self, name: &str) -> ProviderConfig {
        self.try_provider(name).unwrap_or_else(|_| self.providers.get(name).cloned().unwrap_or_default())
    }

    pub fn credential_storage(&self, name: &str) -> CredentialStorage {
        self.credential_storage.get(name).copied().unwrap_or_else(|| {
            if self.credential_refs.contains_key(name) { CredentialStorage::Keychain }
            else { CredentialStorage::File }
        })
    }

    pub fn try_provider(&self, name: &str) -> Result<ProviderConfig, String> {
        let mut provider = self.providers.get(name).cloned().unwrap_or_default();
        if provider.api_key.is_empty() {
            if let Some(reference) = self.credential_refs.get(name) {
                provider.api_key = crate::mac_credentials::read(reference)?;
            }
        }
        Ok(provider)
    }

    pub fn set_provider(&mut self, name: &str, p: ProviderConfig) {
        self.credential_refs.remove(name);
        self.providers.insert(name.to_string(), p);
    }
}

pub fn config_path() -> PathBuf {
    #[cfg(feature = "test-fixtures")]
    {
        let args = std::env::args().collect::<Vec<_>>();
        if let Some(index) = args.iter().position(|arg| arg == "--test-profile") {
            if let Some(root) = args.get(index + 1).map(PathBuf::from).filter(|root| root.is_absolute()) {
                return root.join("config.json");
            }
        }
    }
    // Legacy UI selftests write synthetic credentials then restore JSON. Isolate
    // their entire process so replacing a key cannot invalidate a user's snapshot.
    if std::env::var_os("BEEK_SELFTEST").is_some() {
        static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        return PATH.get_or_init(|| std::env::temp_dir().join(format!("beek-ui-selftest-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos())).join("config.json")).clone();
    }

    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/BeeK/config.json")
}

pub fn load() -> Config {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &Config) {
    let _ = try_save(cfg);
}

fn try_save(cfg: &Config) -> Result<(), String> {
    let p = config_path();
    save_at(cfg, &p)
}

fn save_at(cfg: &Config, p: &std::path::Path) -> Result<(), String> {
    save_at_with_storage(cfg, p, None)
}

fn save_at_with_storage(cfg: &Config, p: &std::path::Path, selected: Option<&str>) -> Result<(), String> {
    static SAVE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = SAVE_LOCK.lock().map_err(|_| "配置保存锁不可用。")?;
    let old: Option<Config> = std::fs::read_to_string(p).ok().and_then(|s|serde_json::from_str(&s).ok());
    let mut persisted = cfg.clone();
    struct Staged { references: Vec<String>, committed: bool }
    impl Drop for Staged {
        fn drop(&mut self) {
            if !self.committed { for reference in &self.references { crate::mac_credentials::delete(reference); } }
        }
    }
    let mut staged = Staged { references: Vec::new(), committed: false };
    let storage = selected.map(|name| cfg.credential_storage(name));
    for (name, provider) in &mut persisted.providers {
        // Only an explicit service save can move its Key. Preference saves and
        // other accounts retain exactly the current storage representation.
        if selected != Some(name.as_str()) { continue; }
        if storage == Some(CredentialStorage::File) {
            persisted.credential_refs.remove(name);
            continue;
        }
        if provider.api_key.is_empty() { continue; }
        let existing = old.as_ref().and_then(|c|c.credential_refs.get(name));
        let reference = if let Some(reference) = existing.filter(|r|
            crate::mac_credentials::read(r).is_ok_and(|key|key == provider.api_key)) {
            reference.clone()
        } else {
            let mut random = [0u8;16];
            getrandom::fill(&mut random).map_err(|_| "无法生成凭据引用。")?;
            let reference = random.iter().map(|b|format!("{b:02x}")).collect::<String>();
            crate::mac_credentials::write(&reference, &provider.api_key)?;
            staged.references.push(reference.clone());
            if crate::mac_credentials::read(&reference)? != provider.api_key {
                return Err("钥匙串读回校验失败，原配置未更改。".into());
            }
            reference
        };
        persisted.credential_refs.insert(name.clone(), reference);
        provider.api_key.clear();
    }

    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&persisted).map_err(|e| e.to_string())?;
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|_| "无法生成配置临时文件名。")?;
    let suffix = random.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let temporary = p.with_file_name(format!(".beek-config-{suffix}.tmp"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let result = (|| {
        use std::io::Write;
        let mut file = options.open(&temporary)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary, p)
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    if result.is_ok() {
        staged.committed = true;
        if let Some(old) = old {
            for reference in old.credential_refs.values() {
                if !persisted.credential_refs.values().any(|r|r == reference) { crate::mac_credentials::delete(reference); }
            }
        }
    }
    result.map_err(|e: std::io::Error| format!("{}{}",beek::core::localization::tr("配置写入失败："),e))
}

pub fn validate_engine_settings(engine: Engine, provider: &ProviderConfig) -> Result<(), String> {
    let mut effective = provider.clone();
    if engine == Engine::Ark && effective.api_key.trim().is_empty() {
        effective.api_key = std::env::var("BEEK_ARK_KEY").unwrap_or_default();
    }
    beek::core::translation_engine::validate(engine, &effective)
}

fn apply_engine_settings(cfg: &mut Config, engine: Engine, provider: ProviderConfig) -> Result<(), String> {
    validate_engine_settings(engine, &provider)?;
    cfg.set_provider(engine.key(), provider);
    cfg.translation_engine = engine;
    Ok(())
}

pub fn save_engine_settings(engine: Engine, provider: ProviderConfig, storage: CredentialStorage) -> Result<(), String> {
    save_engine_settings_at(&config_path(), engine, provider, storage)
}

fn save_engine_settings_at(path: &std::path::Path, engine: Engine, provider: ProviderConfig, storage: CredentialStorage) -> Result<(), String> {
    let mut cfg = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|_| "配置文件损坏，原配置未更改。")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(_) => return Err("无法读取配置，原配置未更改。".into()),
    };
    apply_engine_settings(&mut cfg, engine, provider)?;
    cfg.credential_storage.insert(engine.key().into(), storage);
    save_at_with_storage(&cfg, path, Some(engine.key()))
}

pub fn finish_onboarding() -> Result<(),String> { finish_onboarding_at(&config_path()) }
pub fn save_translation_target(target:TargetLanguage) -> Result<(),String> {
    save_translation_target_at(&config_path(),target)
}
fn save_translation_target_at(path:&std::path::Path,target:TargetLanguage) -> Result<(),String> {
    change_preferences_at(path,|cfg|cfg.translation_target=target)
}
pub fn save_ui_language(language:UiLanguage)->Result<(),String>{
    change_preferences_at(&config_path(),|cfg|cfg.ui_language=language)
}
fn change_preferences_at(path:&std::path::Path,change:impl FnOnce(&mut Config))->Result<(),String>{
    let mut cfg:Config=match std::fs::read_to_string(path) {
        Ok(s)=>serde_json::from_str(&s).map_err(|_|"配置文件损坏，原配置未更改。")?,
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Config::default(),
        Err(_)=>return Err("无法读取配置，原配置未更改。".into()),
    };
    change(&mut cfg);
    save_at(&cfg,path)
}
fn finish_onboarding_at(path:&std::path::Path) -> Result<(),String> {
    let mut cfg:Config=match std::fs::read_to_string(path) {
        Ok(s)=>serde_json::from_str(&s).map_err(|_|"配置文件损坏，原配置未更改。")?,
        Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Config::default(),
        Err(_)=>return Err("无法读取配置，原配置未更改。".into()),
    };
    cfg.onboarding_revision=cfg.onboarding_revision.max(1);
    save_at(&cfg,path)
}

pub fn save_cache_settings(settings: beek::core::translation_cache::CacheSettings) -> Result<(), String> {
    let mut cfg = load();
    cfg.translation_cache = settings.normalized();
    try_save(&cfg)
}

/// 读-改-写。必须用它做局部修改：直接 `save(&Config { show_in_dock })` 会把
/// 用户刚在「设置 API Key…」里配好的 providers 一起抹掉。
pub fn update(f: impl FnOnce(&mut Config)) {
    let mut cfg = load();
    f(&mut cfg);
    save(&cfg);
}

#[cfg(test)]
mod tests {
    use super::*;
    use beek::core::translation_cache::CacheSettings;
    #[test]
    fn language_preferences_preserve_credentials_service_cache_and_onboarding_and_reject_corruption(){
        let dir=std::env::temp_dir().join(format!("beek-language-prefs-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&dir).unwrap();let path=dir.join("config.json");
        let mut cfg=Config::default();cfg.translation_engine=Engine::Ark;cfg.onboarding_revision=1;cfg.translation_cache.max_entries=73;
        cfg.set_provider(ARK,ProviderConfig{api_key:"fictional-preserved-key".into(),..Default::default()});save_at(&cfg,&path).unwrap();
        save_translation_target_at(&path,TargetLanguage::Japanese).unwrap();
        change_preferences_at(&path,|c|c.ui_language=UiLanguage::English).unwrap();
        let saved:Config=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.translation_target,TargetLanguage::Japanese);assert_eq!(saved.ui_language,UiLanguage::English);
        assert_eq!(saved.provider(ARK).api_key,"fictional-preserved-key");assert_eq!(saved.translation_engine,Engine::Ark);assert_eq!(saved.onboarding_revision,1);assert_eq!(saved.translation_cache.max_entries,73);
        let legacy:Config=serde_json::from_str(r#"{"providers":{"ark":{"api_key":"fictional-legacy"}}}"#).unwrap();
        assert_eq!(legacy.translation_target,TargetLanguage::Chinese);assert_eq!(legacy.ui_language,UiLanguage::System);
        std::fs::write(&path,b"{invalid").unwrap();assert!(save_translation_target_at(&path,TargetLanguage::English).is_err());assert_eq!(std::fs::read(&path).unwrap(),b"{invalid");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn onboarding_completion_is_atomic_preserves_settings_and_rejects_corrupt_config() {
        let dir=std::env::temp_dir().join(format!("beek-onboarding-config-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&dir).unwrap();let path=dir.join("config.json");
        let mut cfg=Config::default();cfg.translation_engine=Engine::Ark;
        cfg.set_provider(ARK,ProviderConfig{api_key:"fictional-key".into(),..Default::default()});
        cfg.translation_cache.max_entries=67;save_at(&cfg,&path).unwrap();
        assert_eq!(cfg.onboarding_revision,0);finish_onboarding_at(&path).unwrap();
        let saved:Config=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.onboarding_revision,1);assert_eq!(saved.translation_engine,Engine::Ark);
        assert_eq!(saved.provider(ARK).api_key,"fictional-key");assert_eq!(saved.translation_cache.max_entries,67);
        std::fs::write(&path,b"{broken-config").unwrap();assert!(finish_onboarding_at(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(),b"{broken-config");std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn fresh_users_default_to_public_baidu_without_credentials_and_preserve_legacy_accounts() {
        for cfg in [Config::default(), serde_json::from_str::<Config>("{}").unwrap()] {
            assert_eq!(cfg.translation_engine,Engine::PublicBaidu);
            assert!(cfg.clipboard_fallback && cfg.auto_update);
            assert!(cfg.providers.is_empty());
        }
        let legacy:Config=serde_json::from_str(r#"{"providers":{"ark":{"api_key":"fixture-only"}}}"#).unwrap();
        assert_eq!(legacy.translation_engine,Engine::Ark);
        for engine in Engine::ALL {
            let value=serde_json::json!({"translation_engine":engine,"providers":{"baidu":{"api_key":"fixture-only","app_id":"fixture-app"}}});
            let cfg:Config=serde_json::from_value(value).unwrap();
            assert_eq!(cfg.translation_engine,engine);
            assert_eq!(cfg.provider("baidu").app_id,"fixture-app");
        }
        let cfg:Config=serde_json::from_str(r#"{"translation_engine":"baidu"}"#).unwrap();
        assert_eq!(cfg.translation_engine,Engine::Baidu);
    }

    #[test]
    fn engine_switches_preserve_other_accounts_cache_and_unknown_engine_is_not_ark() {
        let mut cfg: Config = serde_json::from_str(r#"{"providers":{"ark":{"api_key":"fictional-fixture","base_url":"https://example.invalid/v1/chat/completions"}},"translation_cache":{"max_entries":64,"ttl_minutes":10}}"#).unwrap();
        assert_eq!(cfg.translation_engine,Engine::Ark);
        let azure = ProviderConfig { api_key:"azure-fixture".into(),region:"eastasia".into(),..Default::default() };
        apply_engine_settings(&mut cfg,Engine::Azure,azure).unwrap();
        assert_eq!(cfg.translation_engine,Engine::Azure);
        assert_eq!(cfg.provider(ARK).api_key,"fictional-fixture");
        assert_eq!(cfg.translation_cache.max_entries,64);
        let before=serde_json::to_string(&cfg).unwrap();
        assert!(apply_engine_settings(&mut cfg,Engine::Baidu,ProviderConfig::default()).is_err());
        assert_eq!(serde_json::to_string(&cfg).unwrap(),before);
        let unknown: Config=serde_json::from_str(r#"{"translation_engine":"not-a-provider","providers":{"ark":{"api_key":"fictional-fixture"}}}"#).unwrap();
        assert_eq!(unknown.translation_engine,Engine::Unsupported);
        assert_eq!(unknown.provider(ARK).api_key,"fictional-fixture");
    }

    #[test]
    fn atomic_save_round_trips_engine_and_removes_its_own_failed_temporary_file() {
        let dir=std::env::temp_dir().join(format!("beek-config-test-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&dir).unwrap();
        let path=dir.join("config.json");
        let mut cfg=Config::default();
        apply_engine_settings(&mut cfg,Engine::MyMemory,ProviderConfig::default()).unwrap();
        save_at(&cfg,&path).unwrap();
        let loaded:Config=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(loaded.translation_engine,Engine::MyMemory);
        #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,0o600); }
        let directory_target=dir.join("directory");std::fs::create_dir(&directory_target).unwrap();
        assert!(save_at(&cfg,&directory_target).is_err());
        assert!(directory_target.is_dir());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(),2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cache_defaults_and_invalid_cache_fields_preserve_existing_provider() {
        for cache in ["null", r#"{"max_entries":"bad","ttl_minutes":1e100}"#, r#"{"max_entries":-4,"ttl_minutes":0}"#] {
            let json = format!(r#"{{"providers":{{"ark":{{"api_key":"fictional-fixture","model":"fixture-model"}}}},"translation_cache":{cache}}}"#);
            let mut cfg: Config = serde_json::from_str(&json).unwrap();
            cfg.translation_cache = CacheSettings::from_input("64", "10").unwrap();
            let saved = serde_json::to_string(&cfg).unwrap();
            let loaded: Config = serde_json::from_str(&saved).unwrap();
            assert_eq!(loaded.provider(ARK).api_key, "fictional-fixture");
            assert_eq!(loaded.provider(ARK).model.as_deref(), Some("fixture-model"));
            assert_eq!(loaded.translation_cache.max_entries, 64);
        }
        let old: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(old.translation_cache, CacheSettings::default());
    }
}

#[cfg(test)]
mod credential_tests {
    use super::*;
    use crate::mac_credentials as secrets;
    fn fixture() -> (Config, PathBuf) {
        let mut cfg = Config::default();
        cfg.set_provider(ARK, ProviderConfig { api_key: "synthetic-migration-key".into(), ..Default::default() });
        cfg.credential_storage.insert(ARK.into(), CredentialStorage::Keychain);
        let path = std::env::temp_dir().join(format!("beek-secure-{}-{:?}.json", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_file(&path);
        (cfg, path)
    }
    #[test]
    fn migration_preferences_replacement_and_clear_keep_json_secret_free() {
        let (cfg,path)=fixture();
        std::fs::write(&path,serde_json::to_string(&cfg).unwrap()).unwrap();
        save_at_with_storage(&cfg,&path,Some(ARK)).unwrap();
        let raw=std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("synthetic-migration-key"));
        let mut saved:Config=serde_json::from_str(&raw).unwrap();
        assert_eq!(saved.try_provider(ARK).unwrap().api_key,"synthetic-migration-key");
        let old=saved.credential_refs[ARK].clone();
        saved.show_in_dock=true;
        save_at(&saved,&path).unwrap();
        assert_eq!(secrets::read(&old).unwrap(),"synthetic-migration-key");
        saved.set_provider(ARK,ProviderConfig {api_key:"synthetic-replacement".into(),..Default::default()});
        save_at_with_storage(&saved,&path,Some(ARK)).unwrap();
        assert!(secrets::read(&old).is_err());
        let saved:Config=serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let reference=saved.credential_refs[ARK].clone();
        secrets::delete(&reference);
        assert!(saved.try_provider(ARK).is_err()); // No environment fallback on read failure.
        let mut cleared=saved;
        cleared.set_provider(ARK,ProviderConfig::default());
        save_at_with_storage(&cleared,&path,Some(ARK)).unwrap();
        let result:Config=serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(result.credential_refs.is_empty());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn file_is_default_and_preferences_do_not_migrate_any_key() {
        let (mut cfg,path)=fixture();
        cfg.credential_storage.clear();
        assert_eq!(cfg.credential_storage(ARK),CredentialStorage::File);
        let before=secrets::count();
        save_at(&cfg,&path).unwrap();
        save_translation_target_at(&path,TargetLanguage::Japanese).unwrap();
        change_preferences_at(&path,|c| c.ui_language=UiLanguage::English).unwrap();
        finish_onboarding_at(&path).unwrap();
        let saved:Config=serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.providers[ARK].api_key,"synthetic-migration-key");
        assert!(saved.credential_refs.is_empty());
        assert_eq!(secrets::count(),before);
        // Even an unsaved storage draft cannot trigger a migration via preference save.
        cfg.credential_storage.insert(ARK.into(),CredentialStorage::Keychain);
        save_at(&cfg,&path).unwrap();
        assert_eq!(secrets::count(),before);
        assert!(std::fs::read_to_string(&path).unwrap().contains("synthetic-migration-key"));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn explicit_service_save_migrates_only_that_service_and_can_switch_back_to_file() {
        let (mut cfg,path)=fixture();
        cfg.credential_storage.clear();
        cfg.set_provider("azure",ProviderConfig {api_key:"synthetic-azure-key".into(),region:"eastasia".into(),..Default::default()});
        save_at(&cfg,&path).unwrap();
        let provider=cfg.try_provider(ARK).unwrap();
        save_engine_settings_at(&path,Engine::Ark,provider,CredentialStorage::Keychain).unwrap();
        let raw=std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("synthetic-migration-key"));
        assert!(raw.contains("synthetic-azure-key"));
        let saved:Config=serde_json::from_str(&raw).unwrap();
        assert_eq!(saved.credential_storage(ARK),CredentialStorage::Keychain);
        assert_eq!(saved.credential_storage("azure"),CredentialStorage::File);
        let reference=saved.credential_refs[ARK].clone();
        save_engine_settings_at(&path,Engine::Ark,saved.try_provider(ARK).unwrap(),CredentialStorage::File).unwrap();
        let saved:Config=serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(saved.credential_refs.is_empty());
        assert_eq!(saved.providers[ARK].api_key,"synthetic-migration-key");
        assert_eq!(saved.credential_storage(ARK),CredentialStorage::File);
        assert!(secrets::read(&reference).is_err());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn existing_keychain_accounts_keep_their_storage_without_a_new_choice() {
        let cfg:Config=serde_json::from_str(r#"{"providers":{"ark":{}},"credential_refs":{"ark":"fixture-ref"}}"#).unwrap();
        assert_eq!(cfg.credential_storage(ARK),CredentialStorage::Keychain);
        assert_eq!(cfg.credential_storage("azure"),CredentialStorage::File);
    }
    #[test]
    fn failed_migration_keeps_legacy_bytes_and_rolls_back_staged_items() {
        let (cfg,path)=fixture();
        let raw=serde_json::to_string(&cfg).unwrap();
        std::fs::write(&path,&raw).unwrap();
        for operation in [1,2] {
            let before=secrets::count();
            secrets::fail_next(operation);
            assert!(save_at_with_storage(&cfg,&path,Some(ARK)).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(),raw);
            assert_eq!(secrets::count(),before);
        }
        let before=secrets::count();
        let impossible=path.join("cannot-write.json");
        assert!(save_at_with_storage(&cfg,&impossible,Some(ARK)).is_err());
        assert_eq!(secrets::count(),before);
        assert_eq!(std::fs::read_to_string(&path).unwrap(),raw);
        std::fs::remove_file(path).unwrap();
    }
}

/// Bounded native storage transaction check using only an ephemeral synthetic account.
#[cfg(feature = "test-fixtures")]
pub fn credential_storage_selftest() -> Result<(), String> {
    credential_storage_check(false)
}

#[cfg(feature = "test-fixtures")]
pub fn credential_storage_ui_selftest() -> Result<(), String> {
    credential_storage_check(true)
}

#[cfg(feature = "test-fixtures")]
fn credential_storage_check(native_ui: bool) -> Result<(), String> {
    let mut random = [0u8;16];
    getrandom::fill(&mut random).map_err(|_| "无法生成诊断目录。")?;
    let suffix=random.iter().map(|b|format!("{b:02x}")).collect::<String>();
    let directory=std::env::temp_dir().join(format!("beek-storage-check-{suffix}"));
    std::fs::create_dir(&directory).map_err(|_| "无法创建诊断目录。")?;
    let path=directory.join("config.json");
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Ok(raw)=std::fs::read_to_string(self.0.join("config.json")) {
                if let Ok(cfg)=serde_json::from_str::<Config>(&raw) {
                    for reference in cfg.credential_refs.values() { crate::mac_credentials::delete(reference); }
                }
            }
            let _=std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup=Cleanup(directory);
    let provider=ProviderConfig {api_key:"beek-public-synthetic-storage-key".into(),..Default::default()};
    save_engine_settings_at(&path,Engine::Ark,provider,CredentialStorage::File)?;
    let read=|| -> Result<Config,String> {
        let raw=std::fs::read_to_string(&path).map_err(|_| "无法读取诊断配置。")?;
        serde_json::from_str(&raw).map_err(|_| "诊断配置格式错误。".into())
    };
    let file=read()?;
    if file.credential_storage(ARK)!=CredentialStorage::File || file.providers[ARK].api_key!="beek-public-synthetic-storage-key" { return Err("文件保存检查失败。".into()); }
    let (engine,provider,storage)=if native_ui {
        crate::dialogs::storage_choice_selftest(&file,CredentialStorage::Keychain,false).ok_or("保存被意外取消。")?
    } else { (Engine::Ark,file.try_provider(ARK)?,CredentialStorage::Keychain) };
    save_engine_settings_at(&path,engine,provider,storage)?;
    let secured=read()?;
    if !secured.providers[ARK].api_key.is_empty() || secured.try_provider(ARK)?.api_key!="beek-public-synthetic-storage-key" { return Err("钥匙串迁移检查失败。".into()); }
    change_preferences_at(&path,|cfg|cfg.ui_language=UiLanguage::English)?;
    let kept=read()?;
    if kept.credential_refs!=secured.credential_refs || kept.try_provider(ARK)?.api_key!="beek-public-synthetic-storage-key" { return Err("偏好保存改变了凭据。".into()); }
    if native_ui {
        let before=std::fs::read(&path).map_err(|_| "无法读取诊断配置。")?;
        if crate::dialogs::storage_choice_selftest(&kept,CredentialStorage::File,true).is_some() { return Err("取消返回了保存结果。".into()); }
        if std::fs::read(&path).map_err(|_| "无法读取诊断配置。")?!=before { return Err("取消改变了磁盘配置。".into()); }
    }
    let (engine,provider,storage)=if native_ui {
        crate::dialogs::storage_choice_selftest(&kept,CredentialStorage::File,false).ok_or("保存被意外取消。")?
    } else { (Engine::Ark,kept.try_provider(ARK)?,CredentialStorage::File) };
    save_engine_settings_at(&path,engine,provider,storage)?;
    let restored=read()?;
    if !restored.credential_refs.is_empty() || restored.providers[ARK].api_key!="beek-public-synthetic-storage-key" { return Err("切回文件检查失败。".into()); }
    Ok(())
}
