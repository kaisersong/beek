// BeeK 配置模块 — 持久化到 ~/Library/Application Support/BeeK/config.json
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    /// 是否在 Dock 显示图标，默认 false（只显示菜单栏图标）
    #[serde(default)]
    pub show_in_dock: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { show_in_dock: false }
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
