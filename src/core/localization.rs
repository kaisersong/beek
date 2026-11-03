//! UI language is independent of the immutable translation-request language.
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum UiLanguage {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "en")]
    English,
}
impl<'de> Deserialize<'de> for UiLanguage {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        Ok(match v.as_str() {
            Some("zh-CN") => Self::Chinese,
            Some("en") => Self::English,
            _ => Self::System,
        })
    }
}
impl UiLanguage {
    pub const ALL: [Self; 3] = [Self::System, Self::Chinese, Self::English];
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "跟随系统 / System",
            Self::Chinese => "简体中文",
            Self::English => "English",
        }
    }
    pub fn resolves_to_english(self, system: &str) -> bool {
        match self {
            Self::English => true,
            Self::Chinese => false,
            Self::System => !system.to_lowercase().starts_with("zh"),
        }
    }
}
static ENGLISH: AtomicBool = AtomicBool::new(false);
pub fn configure(language: UiLanguage, system: &str) {
    ENGLISH.store(language.resolves_to_english(system), Ordering::SeqCst);
}
pub fn is_english() -> bool {
    ENGLISH.load(Ordering::SeqCst)
}
pub fn choose<'a>(zh: &'a str, en: &'a str) -> &'a str {
    if is_english() {
        en
    } else {
        zh
    }
}
/// Format only app-owned catalog templates. Argument bytes are copied unchanged.
pub fn message(template: &str, values: &[&str]) -> String {
    let mut parts = tr(template).split("{}");
    let mut out = parts.next().unwrap_or_default().to_owned();
    for value in values {
        out.push_str(value);
        out.push_str(parts.next().unwrap_or_default());
    }
    debug_assert!(parts.next().is_none(), "UI message argument count");
    out
}
/// Exact UI-message lookup only: never rewrite substrings in user text or credentials.
pub fn tr(text: &str) -> &str {
    text_for(text, is_english())
}
pub fn text_for(text: &str, english: bool) -> &str {
    if !english {
        return text;
    }
    static EN: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    EN.get_or_init(|| {
        serde_json::from_str(include_str!("../../resources/i18n/en.json"))
            .expect("bundled English UI catalog")
    })
    .get(text)
    .map(String::as_str)
    .unwrap_or(text)
}
pub fn canonical(text: &str) -> &str {
    static EN: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    EN.get_or_init(|| {
        serde_json::from_str(include_str!("../../resources/i18n/en.json"))
            .expect("bundled English UI catalog")
    })
    .iter()
    .find(|(_, v)| v.as_str() == text)
    .map(|(k, _)| k.as_str())
    .unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_and_explicit_ui_languages_are_independent_of_translation_targets() {
        assert!(!UiLanguage::System.resolves_to_english("zh-Hant-CN"));
        assert!(UiLanguage::System.resolves_to_english("en-GB"));
        assert!(UiLanguage::English.resolves_to_english("zh-CN"));
        assert!(!UiLanguage::Chinese.resolves_to_english("en-US"));
        for text in [
            "开始体验",
            "点击试译",
            "翻译服务设置",
            "翻译缓存设置",
            "界面语言",
            "翻译为",
            "保存并启用",
            "取消",
        ] {
            assert_eq!(text_for(text, false), text);
            assert_ne!(
                text_for(text, true),
                text,
                "missing English UI message: {text}"
            );
            assert_eq!(canonical(text_for(text, true)), text);
        }
        assert_eq!(
            text_for("fictional-user-key-not-a-ui-message", true),
            "fictional-user-key-not-a-ui-message"
        );
    }
    #[test]
    fn english_catalog_preserves_message_parameters_and_never_interprets_argument_text() {
        let catalog: BTreeMap<String, String> =
            serde_json::from_str(include_str!("../../resources/i18n/en.json")).unwrap();
        for (zh, en) in catalog {
            assert!(!en.trim().is_empty(), "{zh}");
            assert_eq!(zh.matches("{}").count(), en.matches("{}").count(), "{zh}");
        }
        assert_eq!(
            message("{}\n\n当前版本 v{}。", &["fixture{}raw", "1.1.0"]),
            "fixture{}raw\n\n当前版本 v1.1.0。"
        );
    }
}
