//! Stable user preference; service-specific language codes live at this boundary.
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum TargetLanguage {
    #[default]
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "ja")]
    Japanese,
    #[serde(rename = "ko")]
    Korean,
    #[serde(rename = "zh-TW")]
    TraditionalChinese,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "es")]
    Spanish,
}
impl<'de> Deserialize<'de> for TargetLanguage {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(Self::ALL
            .into_iter()
            .find(|l| Some(l.key()) == value.as_str())
            .unwrap_or_default())
    }
}
impl TargetLanguage {
    pub const ALL: [Self; 8] = [
        Self::Chinese,
        Self::English,
        Self::Japanese,
        Self::Korean,
        Self::TraditionalChinese,
        Self::French,
        Self::German,
        Self::Spanish,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Chinese => "zh-CN",
            Self::English => "en",
            Self::Japanese => "ja",
            Self::Korean => "ko",
            Self::TraditionalChinese => "zh-TW",
            Self::French => "fr",
            Self::German => "de",
            Self::Spanish => "es",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Chinese => "简体中文",
            Self::English => "英文",
            Self::Japanese => "日文",
            Self::Korean => "韩文",
            Self::TraditionalChinese => "繁体中文",
            Self::French => "法文",
            Self::German => "德文",
            Self::Spanish => "西班牙文",
        }
    }
    pub fn baidu(self) -> &'static str {
        match self {
            Self::Chinese => "zh",
            Self::Japanese => "jp",
            Self::Korean => "kor",
            Self::TraditionalChinese => "cht",
            Self::French => "fra",
            Self::Spanish => "spa",
            _ => self.key(),
        }
    }
    pub fn azure(self) -> &'static str {
        match self {
            Self::Chinese => "zh-Hans",
            Self::TraditionalChinese => "zh-Hant",
            _ => self.key(),
        }
    }
    pub fn libre(self) -> &'static str {
        match self {
            Self::Chinese => "zh",
            Self::TraditionalChinese => "zt",
            _ => self.key(),
        }
    }
}
