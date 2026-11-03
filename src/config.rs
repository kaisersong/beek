// BeeK 配置模块 — 持久化到 ~/Library/Application Support/BeeK/config.json
//
// 两件事都在这里：界面偏好（是否显示 Dock 图标）和翻译服务的凭据（API Key / 模型）。
// 凭据放 map 而不是固定字段：现在只有豆包 Ark 一家，以后加服务不用改配置格式，
// 也不会因为老配置缺字段而读不出来（每个字段都有 serde default）。
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

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
        Self { translation_target:w.translation_target, ui_language:w.ui_language, show_in_dock:w.show_in_dock, clipboard_fallback:w.clipboard_fallback,
            copy_to_popup:w.copy_to_popup, auto_update:w.auto_update,
            translation_cache:w.translation_cache,translation_engine,providers:w.providers,onboarding_revision:w.onboarding_revision }
    }
}
impl Default for Config {
    fn default() -> Self {
        Self {translation_target:TargetLanguage::default(),ui_language:UiLanguage::default(),show_in_dock:false,clipboard_fallback:true,copy_to_popup:false,auto_update:true,
            translation_cache:Default::default(),translation_engine:Engine::PublicBaidu,providers:Default::default(),onboarding_revision:0}
    }
}

impl Config {
    pub fn provider(&self, name: &str) -> ProviderConfig {
        self.providers.get(name).cloned().unwrap_or_default()
    }

    pub fn set_provider(&mut self, name: &str, p: ProviderConfig) {
        self.providers.insert(name.to_string(), p);
    }
}

pub fn config_path() -> PathBuf {
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
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
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

pub fn save_engine_settings(engine: Engine, provider: ProviderConfig) -> Result<(), String> {
    let mut cfg = match std::fs::read_to_string(config_path()) {
        Ok(text) => serde_json::from_str(&text).map_err(|_| "配置文件损坏，原配置未更改。")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(_) => return Err("无法读取配置，原配置未更改。".into()),
    };
    apply_engine_settings(&mut cfg, engine, provider)?;
    try_save(&cfg)
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
