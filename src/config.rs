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

/// 单个翻译服务的配置
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ProviderConfig {
    /// API Key。空表示没配（此时回落到环境变量）
    #[serde(default)]
    pub api_key: String,
    /// 模型 / 推理接入点 ID。空表示用代码里的默认模型
    #[serde(default)]
    pub model: Option<String>,
    /// 自定义端点（OpenAI 兼容服务用；Ark 留空）
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Config {
    /// 是否在 Dock 显示图标，默认 false（只显示菜单栏图标）
    #[serde(default)]
    pub show_in_dock: bool,
    /// 各翻译服务的配置，键是服务名（见 ARK）
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
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
    let p = config_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(&p, json);
    }
}

/// 读-改-写。必须用它做局部修改：直接 `save(&Config { show_in_dock })` 会把
/// 用户刚在「设置 API Key…」里配好的 providers 一起抹掉。
pub fn update(f: impl FnOnce(&mut Config)) {
    let mut cfg = load();
    f(&mut cfg);
    save(&cfg);
}
