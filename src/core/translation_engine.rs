//! Provider identities and bounded, non-streaming translation protocols.
use super::translation_language::TargetLanguage;
use md5::{Digest, Md5};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};
use std::io::Read;
use std::time::Duration;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    #[default]
    Ark,
    Azure,
    Baidu,
    #[serde(rename = "public-baidu")]
    PublicBaidu,
    Google,
    MyMemory,
    LibreTranslate,
    Unsupported,
}
impl<'de> Deserialize<'de> for Engine {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        Ok(match v.as_str() {
            Some("ark") => Self::Ark,
            Some("azure") => Self::Azure,
            Some("baidu") => Self::Baidu,
            Some("public-baidu") => Self::PublicBaidu,
            Some("google") => Self::Google,
            Some("mymemory") => Self::MyMemory,
            Some("libretranslate") => Self::LibreTranslate,
            _ => Self::Unsupported,
        })
    }
}
impl Engine {
    pub const ALL: [Self; 7] = [
        Self::PublicBaidu,
        Self::Ark,
        Self::Azure,
        Self::Baidu,
        Self::Google,
        Self::MyMemory,
        Self::LibreTranslate,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Ark => "ark",
            Self::Azure => "azure",
            Self::Baidu => "baidu",
            Self::PublicBaidu => "public-baidu",
            Self::Google => "google",
            Self::MyMemory => "mymemory",
            Self::LibreTranslate => "libretranslate",
            Self::Unsupported => "unsupported",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Ark => "豆包 / OpenAI 兼容",
            Self::Azure => "Microsoft Azure Translator",
            Self::Baidu => "百度翻译（自己的 Key）",
            Self::PublicBaidu => "百度翻译（免费公共服务）",
            Self::Google => "Google Cloud Translation",
            Self::MyMemory => "MyMemory",
            Self::LibreTranslate => "LibreTranslate / Argos",
            Self::Unsupported => "未知引擎（请重新选择）",
        }
    }
}

// Deliberately no Debug: credentials must not enter diagnostics.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineConfig {
    #[serde(skip)]
    pub target_language: TargetLanguage,
    pub api_key: String,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub app_id: String,
    pub region: String,
    pub source_language: String,
    pub email: String,
}

/// Public address only; shared credentials are exclusively Worker secrets.
pub const PUBLIC_BAIDU_ENDPOINT: &str = match option_env!("BEEK_PUBLIC_TRANSLATE_URL") {
    Some(url) => url,
    None => "https://beek-translation-gateway.songkaixuan.workers.dev/v1/translate",
};

pub fn endpoint(engine: Engine, cfg: &EngineConfig) -> String {
    match engine {
        Engine::Ark => cfg
            .base_url
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| super::prompts::DEFAULT_ENDPOINT.into()),
        Engine::Azure => "https://api.cognitive.microsofttranslator.com/translate".into(),
        Engine::Baidu => "https://fanyi-api.baidu.com/api/trans/vip/translate".into(),
        Engine::PublicBaidu => PUBLIC_BAIDU_ENDPOINT.into(),
        Engine::Google => "https://translation.googleapis.com/language/translate/v2".into(),
        Engine::MyMemory => "https://api.mymemory.translated.net/get".into(),
        Engine::LibreTranslate => {
            let value = cfg
                .base_url
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("http://127.0.0.1:5000/translate")
                .trim();
            if let Ok(mut url) = reqwest::Url::parse(value) {
                if url.path() == "/" {
                    url.set_path("/translate");
                }
                url.to_string()
            } else {
                value.into()
            }
        }
        Engine::Unsupported => String::new(),
    }
}

pub fn validate(engine: Engine, cfg: &EngineConfig) -> Result<(), String> {
    if engine == Engine::PublicBaidu {
        let url = reqwest::Url::parse(PUBLIC_BAIDU_ENDPOINT)
            .map_err(|_| "公共翻译服务尚未配置，请选择自己的百度 Key 或其他服务。")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("公共翻译服务地址无效。".into());
        }
    }
    if engine == Engine::Unsupported {
        return Err("未知翻译引擎，请重新选择翻译服务。".into());
    }
    if [
        cfg.api_key.len(),
        cfg.app_id.len(),
        cfg.region.len(),
        cfg.source_language.len(),
        cfg.email.len(),
    ]
    .into_iter()
    .any(|n| n > 4096)
        || cfg.base_url.as_ref().is_some_and(|s| s.len() > 4096)
        || cfg.model.as_ref().is_some_and(|s| s.len() > 1024)
    {
        return Err("翻译服务配置过长。".into());
    }
    if matches!(
        engine,
        Engine::Ark | Engine::Azure | Engine::Baidu | Engine::Google
    ) && cfg.api_key.trim().is_empty()
    {
        return Err("请先填写该翻译服务的 API Key / 密钥。".into());
    }
    if engine == Engine::Baidu && cfg.app_id.trim().is_empty() {
        return Err("百度翻译需要 App ID 和密钥。".into());
    }
    if cfg.api_key.chars().any(|c| c == '\r' || c == '\n')
        || cfg.region.chars().any(|c| c == '\r' || c == '\n')
    {
        return Err("凭据或区域格式无效。".into());
    }
    if engine == Engine::MyMemory
        && !cfg.source_language.is_empty()
        && (!cfg
            .source_language
            .chars()
            .all(|c| c.is_ascii_alphabetic() || c == '-')
            || cfg.source_language.len() > 16
            || cfg.source_language == "auto")
    {
        return Err("MyMemory 需要明确的源语言代码，例如 en、ja；不能使用 auto。".into());
    }
    if matches!(engine, Engine::Ark | Engine::LibreTranslate) {
        let address = endpoint(engine, cfg);
        let u = reqwest::Url::parse(&address).map_err(|_| "翻译服务地址无效。")?;
        if u.scheme() != "https"
            && !(u.scheme() == "http"
                && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")))
        {
            return Err("远端翻译服务需使用 HTTPS；本机可使用 HTTP。".into());
        }
        if u.host_str().is_none()
            || !u.username().is_empty()
            || u.password().is_some()
            || u.fragment().is_some()
        {
            return Err("服务地址不能包含账号、密码或片段。".into());
        }
    }
    Ok(())
}

pub fn baidu_sign(app_id: &str, text: &str, salt: &str, key: &str) -> String {
    let mut digest = Md5::new();
    for s in [app_id, text, salt, key] {
        digest.update(s.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

/// Shared production request builder; endpoint override is used by local HTTP tests.
pub fn build_request(
    client: &reqwest::blocking::Client,
    engine: Engine,
    cfg: &EngineConfig,
    text: &str,
    address: &str,
) -> Result<reqwest::blocking::Request, String> {
    validate(engine, cfg)?;
    if text.trim().is_empty() || text.len() > 32768 {
        return Err("选区为空或超过翻译长度限制。".into());
    }
    let request = match engine {
        Engine::PublicBaidu => {
            if text.len() > 1500 {
                return Err("免费公共翻译单次最多 1500 字节，请缩短选区或切换自己的 Key。".into());
            }
            client
                .post(address)
                .json(&json!({"text":text,"target":cfg.target_language.key()}))
        }
        Engine::Azure => {
            let mut r = client
                .post(address)
                .query(&[("api-version", "3.0"), ("to", cfg.target_language.azure())])
                .header("Ocp-Apim-Subscription-Key", cfg.api_key.trim())
                .json(&json!([{"Text":text}]));
            if !cfg.region.trim().is_empty() {
                r = r.header("Ocp-Apim-Subscription-Region", cfg.region.trim());
            }
            r
        }
        Engine::Baidu => {
            if text.len() > 6000 {
                return Err("百度翻译单次最多支持 6000 字节，请缩短选区。".into());
            }
            let mut random = [0u8; 16];
            getrandom::fill(&mut random).map_err(|_| "系统随机数不可用。")?;
            let salt = random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            let sign = baidu_sign(cfg.app_id.trim(), text, &salt, cfg.api_key.trim());
            client.post(address).form(&[
                ("q", text),
                ("from", "auto"),
                ("to", cfg.target_language.baidu()),
                ("appid", cfg.app_id.trim()),
                ("salt", &salt),
                ("sign", &sign),
            ])
        }
        Engine::Google => client
            .post(address)
            .header("x-goog-api-key", cfg.api_key.trim())
            .json(&json!({"q":text,"target":cfg.target_language.key(),"format":"text"})),
        Engine::MyMemory => {
            if text.len() > 500 {
                return Err("MyMemory 单次最多支持 500 字节，请缩短选区或切换服务。".into());
            }
            let pair = format!(
                "{}|{}",
                if cfg.source_language.trim().is_empty() {
                    "en"
                } else {
                    cfg.source_language.trim()
                },
                cfg.target_language.key()
            );
            let mut r = client
                .get(address)
                .query(&[("q", text), ("langpair", pair.as_str())]);
            if !cfg.email.trim().is_empty() {
                r = r.query(&[("de", cfg.email.trim())]);
            }
            r
        }
        Engine::LibreTranslate => {
            let mut body = json!({"q":text,"source":"auto","target":cfg.target_language.libre(),"format":"text"});
            if !cfg.api_key.trim().is_empty() {
                body["api_key"] = json!(cfg.api_key.trim());
            }
            client.post(address).json(&body)
        }
        _ => return Err("该引擎不使用此翻译协议。".into()),
    };
    request
        .build()
        .map_err(|_| "翻译请求参数无效，请检查服务配置。".into())
}

pub fn execute(
    engine: Engine,
    cfg: &EngineConfig,
    text: &str,
    address: &str,
) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法创建翻译连接。")?;
    let request = build_request(&client, engine, cfg, text, address)?;
    let resp = client.execute(request).map_err(|e| {
        if e.is_timeout() {
            "翻译请求超时，请重试。"
        } else {
            "翻译连接失败，请检查网络与服务地址。"
        }
    })?;
    let status = resp.status();
    if !status.is_success() {
        if engine == Engine::PublicBaidu && status.as_u16() == 503 {
            return Err(
                "百度公共服务暂不可用，请在翻译服务设置中切换自己的百度 Key 或其他引擎。".into(),
            );
        }
        return Err(match status.as_u16() {
            401 | 403 => "翻译服务认证失败，请检查凭据与区域。".into(),
            429 => "翻译额度或频率受限，请稍后重试或手动切换服务。".into(),
            _ => super::localization::message(
                "翻译服务返回 HTTP {}。",
                &[&status.as_u16().to_string()],
            ),
        });
    }
    let mut bytes = Vec::new();
    resp.take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "读取翻译结果失败。")?;
    if bytes.len() > 256 * 1024 {
        return Err("翻译服务响应超过大小限制。".into());
    }
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| "翻译服务返回了无效数据。")?;
    decode(engine, body)
}

pub fn decode(engine: Engine, v: Value) -> Result<String, String> {
    let invalid = || "翻译服务返回错误或无效结果，请检查配置及额度。".to_owned();
    if v.get("error").is_some() || v.get("error_code").is_some() {
        return Err(invalid());
    }
    let text = match engine {
        Engine::Azure => v
            .get(0)
            .and_then(|a| a.get("translations"))
            .and_then(|a| a.get(0))
            .and_then(|a| a.get("text"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        Engine::Baidu => v
            .get("trans_result")
            .and_then(Value::as_array)
            .and_then(|a| {
                a.iter()
                    .map(|t| t.get("dst").and_then(Value::as_str))
                    .collect::<Option<Vec<_>>>()
            })
            .map(|a| a.join("\n")),
        Engine::Google => v
            .pointer("/data/translations/0/translatedText")
            .and_then(Value::as_str)
            .map(|s| html_escape::decode_html_entities(s).into_owned()),
        Engine::PublicBaidu | Engine::LibreTranslate => v
            .get("translatedText")
            .and_then(Value::as_str)
            .map(str::to_owned),
        Engine::MyMemory => {
            let status = v.get("responseStatus").and_then(|s| {
                s.as_i64()
                    .or_else(|| s.as_str().and_then(|s| s.parse().ok()))
            });
            if status != Some(200) || v.get("quotaFinished").and_then(Value::as_bool) == Some(true)
            {
                return Err(invalid());
            }
            v.pointer("/responseData/translatedText")
                .and_then(Value::as_str)
                .map(str::to_owned)
        }
        _ => None,
    }
    .ok_or_else(invalid)?;
    if text.trim().is_empty() || text.len() > 65536 {
        return Err("译文为空或超过大小限制。".into());
    }
    Ok(text.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn public_baidu_never_sends_own_key_and_bounds_utf8_input() {
        if PUBLIC_BAIDU_ENDPOINT.is_empty() {
            return;
        }
        let cfg = EngineConfig {
            api_key: "fictional-user-key".into(),
            app_id: "fictional-user-app".into(),
            ..Default::default()
        };
        let client = reqwest::blocking::Client::new();
        let request = build_request(
            &client,
            Engine::PublicBaidu,
            &cfg,
            "Public sentence.",
            PUBLIC_BAIDU_ENDPOINT,
        )
        .unwrap();
        assert_eq!(request.url().as_str(), PUBLIC_BAIDU_ENDPOINT);
        assert!(!request.headers().contains_key("Authorization"));
        let body = std::str::from_utf8(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(body).unwrap(),
            json!({"text":"Public sentence.","target":"zh-CN"})
        );
        assert!(!body.contains("fictional"));
        assert!(build_request(
            &client,
            Engine::PublicBaidu,
            &cfg,
            &"中".repeat(501),
            PUBLIC_BAIDU_ENDPOINT
        )
        .is_err());
        assert_eq!(
            decode(Engine::PublicBaidu, json!({"translatedText":"公开句子。"})).unwrap(),
            "公开句子。"
        );
        assert!(decode(Engine::PublicBaidu, json!({"error":"monthly_quota"})).is_err());
    }

    #[test]
    fn production_requests_use_each_providers_protocol_without_ai_prompts() {
        let client = reqwest::blocking::Client::new();
        let text = "A & B 中文?";
        let cfg = EngineConfig {
            api_key: "fixture-secret".into(),
            app_id: "fixture-app".into(),
            region: "eastasia".into(),
            source_language: "en".into(),
            email: "fixture@example.invalid".into(),
            ..Default::default()
        };
        for engine in [
            Engine::Azure,
            Engine::Baidu,
            Engine::Google,
            Engine::MyMemory,
            Engine::LibreTranslate,
        ] {
            let request = build_request(
                &client,
                engine,
                &cfg,
                text,
                "http://127.0.0.1:12345/translate",
            )
            .unwrap();
            let query = request
                .url()
                .query_pairs()
                .into_owned()
                .collect::<std::collections::BTreeMap<_, _>>();
            let body = request
                .body()
                .and_then(|b| b.as_bytes())
                .unwrap_or_default();
            match engine {
                Engine::Azure => {
                    assert_eq!(
                        request.headers()["Ocp-Apim-Subscription-Key"],
                        "fixture-secret"
                    );
                    assert_eq!(
                        request.headers()["Ocp-Apim-Subscription-Region"],
                        "eastasia"
                    );
                    assert_eq!(query["to"], "zh-Hans");
                    assert_eq!(
                        serde_json::from_slice::<Value>(body).unwrap()[0]["Text"],
                        text
                    );
                }
                Engine::Baidu => {
                    let form = reqwest::Url::parse(&format!(
                        "http://localhost/?{}",
                        String::from_utf8_lossy(body)
                    ))
                    .unwrap()
                    .query_pairs()
                    .into_owned()
                    .collect::<std::collections::BTreeMap<_, _>>();
                    assert_eq!(form["q"], text);
                    assert_eq!(
                        form["sign"],
                        baidu_sign(&cfg.app_id, text, &form["salt"], &cfg.api_key)
                    );
                    assert!(!String::from_utf8_lossy(body).contains("fixture-secret"));
                }
                Engine::Google => {
                    assert_eq!(request.headers()["x-goog-api-key"], "fixture-secret");
                    assert_eq!(serde_json::from_slice::<Value>(body).unwrap()["q"], text);
                    assert!(!query.contains_key("key"));
                }
                Engine::MyMemory => {
                    assert_eq!(query["q"], text);
                    assert_eq!(query["langpair"], "en|zh-CN");
                    assert_eq!(query["de"], "fixture@example.invalid");
                }
                Engine::LibreTranslate => {
                    let body: Value = serde_json::from_slice(body).unwrap();
                    assert_eq!(body["q"], text);
                    assert_eq!(body["source"], "auto");
                    assert_eq!(body["api_key"], "fixture-secret");
                }
                _ => unreachable!(),
            }
        }
        assert!(build_request(
            &client,
            Engine::MyMemory,
            &cfg,
            &"中".repeat(167),
            "http://127.0.0.1/"
        )
        .is_err());
        assert!(build_request(
            &client,
            Engine::Baidu,
            &cfg,
            &"中".repeat(2001),
            "http://127.0.0.1/"
        )
        .is_err());
    }

    #[test]
    fn selected_targets_reach_each_real_protocol_with_provider_specific_codes() {
        let client = reqwest::blocking::Client::new();
        for (target, baidu, azure, libre, standard) in [
            (TargetLanguage::Japanese, "jp", "ja", "ja", "ja"),
            (
                TargetLanguage::TraditionalChinese,
                "cht",
                "zh-Hant",
                "zt",
                "zh-TW",
            ),
            (TargetLanguage::English, "en", "en", "en", "en"),
        ] {
            let cfg = EngineConfig {
                target_language: target,
                api_key: "fixture".into(),
                app_id: "fixture".into(),
                source_language: "fr".into(),
                ..Default::default()
            };
            for engine in Engine::ALL.into_iter().filter(|e| *e != Engine::Ark) {
                let r = build_request(
                    &client,
                    engine,
                    &cfg,
                    "Public example.",
                    "http://127.0.0.1/translate",
                )
                .unwrap();
                let query = r
                    .url()
                    .query_pairs()
                    .into_owned()
                    .collect::<std::collections::BTreeMap<_, _>>();
                let bytes = r.body().and_then(|b| b.as_bytes()).unwrap_or_default();
                match engine {
                    Engine::PublicBaidu | Engine::Google => assert_eq!(
                        serde_json::from_slice::<Value>(bytes).unwrap()["target"],
                        standard
                    ),
                    Engine::Azure => assert_eq!(query["to"], azure),
                    Engine::Baidu => {
                        let form = reqwest::Url::parse(&format!(
                            "http://localhost/?{}",
                            String::from_utf8_lossy(bytes)
                        ))
                        .unwrap()
                        .query_pairs()
                        .into_owned()
                        .collect::<std::collections::BTreeMap<_, _>>();
                        assert_eq!(form["to"], baidu);
                    }
                    Engine::MyMemory => assert_eq!(query["langpair"], format!("fr|{standard}")),
                    Engine::LibreTranslate => assert_eq!(
                        serde_json::from_slice::<Value>(bytes).unwrap()["target"],
                        libre
                    ),
                    _ => unreachable!(),
                }
            }
        }
    }

    #[test]
    fn old_configs_default_to_ark_and_unknown_engines_fail_closed() {
        assert_eq!(Engine::default(), Engine::Ark);
        let unknown: Engine = serde_json::from_str("\"unknown-provider\"").unwrap();
        assert_eq!(unknown, Engine::Unsupported);
        assert!(validate(unknown, &EngineConfig::default()).is_err());
        assert!(Engine::ALL.iter().all(|e| serde_json::from_str::<Engine>(
            &serde_json::to_string(e).unwrap()
        )
        .unwrap()
            == *e));
    }

    #[test]
    fn validators_require_only_the_selected_engines_credentials() {
        let empty = EngineConfig::default();
        for e in [Engine::Ark, Engine::Azure, Engine::Baidu, Engine::Google] {
            assert!(validate(e, &empty).is_err());
        }
        assert!(validate(Engine::MyMemory, &empty).is_ok());
        assert!(validate(Engine::LibreTranslate, &empty).is_ok());
        let mut cfg = EngineConfig {
            api_key: "fixture-secret".into(),
            ..Default::default()
        };
        assert!(validate(Engine::Baidu, &cfg).is_err());
        cfg.app_id = "fixture-app".into();
        assert!(validate(Engine::Baidu, &cfg).is_ok());
        for url in [
            "file:///tmp/data",
            "http://example.com/translate",
            "https://user:pass@example.com/translate",
            "https://example.com/translate#fragment",
        ] {
            cfg.base_url = Some(url.into());
            assert!(validate(Engine::LibreTranslate, &cfg).is_err());
        }
        cfg.base_url = Some("http://127.0.0.1:5000/translate".into());
        assert!(validate(Engine::LibreTranslate, &cfg).is_ok());
    }

    #[test]
    fn provider_responses_decode_complete_text_and_reject_business_errors() {
        assert_eq!(
            decode(Engine::Azure, json!([{"translations":[{"text":"你好"}]}])).unwrap(),
            "你好"
        );
        assert_eq!(
            decode(
                Engine::Baidu,
                json!({"trans_result":[{"dst":"第一行"},{"dst":"第二行"}]})
            )
            .unwrap(),
            "第一行\n第二行"
        );
        assert_eq!(
            decode(
                Engine::Google,
                json!({"data":{"translations":[{"translatedText":"A &amp; B &#39;x&#39;"}]}})
            )
            .unwrap(),
            "A & B 'x'"
        );
        assert_eq!(
            decode(Engine::LibreTranslate, json!({"translatedText":"你好"})).unwrap(),
            "你好"
        );
        assert_eq!(
            decode(
                Engine::MyMemory,
                json!({"responseStatus":200,"responseData":{"translatedText":"你好"}})
            )
            .unwrap(),
            "你好"
        );
        for (engine, body) in [
            (
                Engine::Baidu,
                json!({"error_code":"54003","error_msg":"fixture-secret"}),
            ),
            (Engine::Azure, json!({"error":{"message":"fixture-secret"}})),
            (
                Engine::Google,
                json!({"error":{"message":"fixture-secret"}}),
            ),
            (Engine::LibreTranslate, json!({"error":"fixture-secret"})),
            (
                Engine::MyMemory,
                json!({"responseStatus":429,"responseData":{"translatedText":"quota exceeded"}}),
            ),
        ] {
            let err = decode(engine, body).unwrap_err();
            assert!(!err.contains("fixture-secret"));
        }
        assert!(decode(Engine::LibreTranslate, json!({"translatedText":" "})).is_err());
        assert!(decode(
            Engine::LibreTranslate,
            json!({"translatedText":"x".repeat(65537)})
        )
        .is_err());
    }

    #[test]
    fn baidu_signs_unencoded_text_with_a_frozen_known_vector() {
        assert_eq!(
            baidu_sign("2015063000000001", "apple", "1435660288", "12345678"),
            "f89f9594663708c1605f3d736d01d2d4"
        );
    }
}
