pub use beek::core::prompts::{is_single_word,split_pron};
use beek::core::prompts::{translate_prompt_for,explain_prompt_for};
#[cfg(test)]
use beek::core::prompts::{translate_prompt,explain_prompt};
// 翻译模块 — 豆包 Ark API（流式 + 关闭思维链）
//
// 实测（短句，本机）：
//   非流式 + 思维链开启  3.12s   ← 旧行为，弹窗要等这么久
//   非流式 + 关闭思维链  1.09s
//   流式   + 关闭思维链  0.62s 见到首字
// 因此这里固定关闭 thinking 并走流式，让弹窗能立刻显示译文增量。
//
// 两种模式：
//   translate_*  选中一段话 → 只出译文
//   explain_*    选中单个词 → 词典式详解（首行是读音，其余是词性/释义/例句/搭配）
use serde::{Deserialize, Serialize};
use beek::core::translation_cache::{CacheSettings, TranslationCache};
use beek::core::translation_engine::{self as engines, Engine, EngineConfig};
use sha2::{Digest, Sha256};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static CACHE: OnceLock<Mutex<TranslationCache>> = OnceLock::new();
fn cache() -> &'static Mutex<TranslationCache> {
    CACHE.get_or_init(|| Mutex::new(TranslationCache::default()))
}

pub fn configure_cache(settings: CacheSettings) {
    cache().lock().unwrap_or_else(|e| e.into_inner()).apply_settings(settings, Instant::now());
}

/// Immutable request identity: the cache and HTTP request use the same settings.
/// Never log this object; it contains the credential for the request.
pub struct TranslationRequest {
    prompt: String,
    endpoint: String,
    model: String,
    key: String,
    cache_key: [u8; 32],
    cache_settings: fn() -> CacheSettings,
    engine: Engine,
    engine_config: EngineConfig,
    text: String,
    dictionary: bool,
}

impl TranslationRequest {
    fn new(prompt: String, endpoint: String, model: String, key: String) -> Self {
        let mut digest = Sha256::new();
        for field in [&prompt, &endpoint, &model, &key] {
            digest.update((field.len() as u64).to_le_bytes());
            digest.update(field.as_bytes());
        }
        Self { prompt, endpoint, model, key, cache_key: digest.finalize().into(), cache_settings: || crate::config::load().translation_cache, engine: Engine::Ark, engine_config: EngineConfig::default(), text: String::new(), dictionary: false }
    }

    pub fn prepare(text: &str, explain: bool) -> Result<Self, String> {
        Self::from_config(&crate::config::load(), text, explain)
    }

    pub(crate) fn from_config(config: &crate::config::Config, text: &str, explain: bool) -> Result<Self, String> {
        let engine = config.translation_engine;
        let mut cfg = config.provider(engine.key());
        cfg.target_language = config.translation_target;
        if engine == Engine::Ark && cfg.api_key.trim().is_empty() {
            cfg.api_key = std::env::var("BEEK_ARK_KEY").unwrap_or_default().trim().to_owned();
        }
        engines::validate(engine, &cfg)?;
        let dictionary = engine == Engine::Ark && explain;
        let prompt = if engine == Engine::Ark {
            if dictionary { explain_prompt_for(text,config.translation_target) } else { translate_prompt_for(text,config.translation_target) }
        } else { text.to_owned() };
        let model = if engine == Engine::Ark {
            cfg.model.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| DEFAULT_MODEL.into())
        } else { engine.key().into() };
        let mut request = Self::new(prompt, engines::endpoint(engine, &cfg), model, cfg.api_key.trim().to_owned());
        let mut digest = Sha256::new();
        digest.update(request.cache_key);
        for value in [engine.key(), cfg.target_language.key(), &cfg.app_id, &cfg.region, &cfg.source_language, &cfg.email] {
            digest.update((value.len() as u64).to_le_bytes()); digest.update(value.as_bytes());
        }
        request.cache_key = digest.finalize().into();
        request.engine = engine;
        request.engine_config = cfg;
        request.text = text.to_owned();
        request.dictionary = dictionary;
        Ok(request)
    }

    pub fn engine_label(&self) -> &'static str { self.engine.label() }

    pub fn is_dictionary(&self) -> bool { self.dictionary }

    pub fn cached(&self) -> Option<String> {
        let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
        let settings = (self.cache_settings)();
        let now = Instant::now();
        cache.apply_settings(settings, now);
        cache.get(&self.cache_key, now)
    }

    fn start(&self, stream: bool) -> Result<reqwest::blocking::Response, String> {
        let resp = client()?.post(&self.endpoint)
            .bearer_auth(&self.key)
            .json(&Req {
                model: self.model.clone(),
                messages: vec![Msg { role: "user".into(), content: self.prompt.clone() }],
                stream,
                thinking: Thinking { kind: "disabled" },
            }).send().map_err(|e| format!("{}{}",beek::core::localization::tr("请求失败："),e))?;
        if !resp.status().is_success() {
            let code = resp.status();
            let body: String = resp.text().unwrap_or_default().chars().take(200).collect();
            return Err(format!("HTTP {} {}", code, body));
        }
        Ok(resp)
    }

    pub fn stream(&self, mut on_chunk: impl FnMut(&str)) -> Result<String, String> {
        if let Some(result) = self.cached() {
            on_chunk(&result);
            return Ok(result);
        }
        let result = if self.engine == Engine::Ark { stream(self, on_chunk)? } else {
            let result = engines::execute(self.engine, &self.engine_config, &self.text, &self.endpoint)?;
            on_chunk(&result);
            result
        };
        let mut cache = cache().lock().unwrap_or_else(|e| e.into_inner());
        let settings = (self.cache_settings)();
        let now = Instant::now();
        cache.apply_settings(settings, now);
        cache.insert(self.cache_key, result.clone(), now);
        Ok(result)
    }
}

const ARK_URL: &str = "https://ark.cn-beijing.volces.com/api/v3/chat/completions";
/// 默认模型（可在「设置 API Key…」里改 providers.ark.model 覆盖）
const DEFAULT_MODEL: &str = "doubao-seed-2-0-mini-260428";
/// 服务名：对应 config.json 里 providers 的键
const PROVIDER: &str = crate::config::ARK;
const TIMEOUT_SECS: u64 = 60;

#[derive(Serialize, Deserialize)]
struct Msg { role: String, content: String }

#[derive(Serialize)]
struct Thinking { #[serde(rename = "type")] kind: &'static str }

#[derive(Serialize)]
struct Req {
    model: String,
    messages: Vec<Msg>,
    stream: bool,
    thinking: Thinking,
}

// 流式增量：只关心正文 content，reasoning_content 已随 thinking 关闭而不出现
#[derive(Deserialize)]
struct Delta { content: Option<String> }
#[derive(Deserialize)]
struct StreamChoice { delta: Delta, finish_reason: Option<String> }
#[derive(Deserialize)]
struct StreamChunk { choices: Vec<StreamChoice> }

// 非流式结构（自检对比用）
#[derive(Deserialize)]
struct BlockChoice { message: Msg }
#[derive(Deserialize)]
struct Resp { choices: Vec<BlockChoice> }

/// 选中的是不是「单个词」：没有空白、不太长、至少含一个字母（拉丁字母或汉字）。
/// 选中单个词时给词典式详解（含读音），而不是只给一句译文。
fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())
}

/// 流式请求：每收到一段正文增量就回调一次 `on_chunk`，返回完整正文。
fn stream(request: &TranslationRequest, mut on_chunk: impl FnMut(&str)) -> Result<String, String> {
    let debug = std::env::var("BEEK_DEBUG").is_ok();
    let t0 = std::time::Instant::now();
    let resp = request.start(true)?;
    let mut done = false;
    let mut finish_reason = None;
    if debug {
        eprintln!(
            "[beek] 流式响应：status={} http={:?} encoding={:?} len={:?}",
            resp.status(),
            resp.version(),
            resp.headers().get("content-encoding"),
            resp.content_length()
        );
    }
    let mut full = String::new();
    let mut parsed_any = false;
    let mut raw_lines = 0usize;
    let mut bytes = 0usize;
    let mut first_raw: Option<u128> = None;
    let mut first_content: Option<u128> = None;
    for line in std::io::BufRead::lines(std::io::BufReader::new(resp)) {
        let line = line.map_err(|e| format!("{}{}",beek::core::localization::tr("读取流失败："),e))?;
        raw_lines += 1;
        bytes += line.len();
        if first_raw.is_none() {
            first_raw = Some(t0.elapsed().as_millis());
            if debug {
                eprintln!(
                    "[beek] 流式首行 {}ms：{}",
                    t0.elapsed().as_millis(),
                    line.chars().take(120).collect::<String>()
                );
            }
        }
        let data = match line.strip_prefix("data:") {
            Some(d) => d.trim(),
            None => continue,
        };
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            done = true;
            break;
        }
        match serde_json::from_str::<StreamChunk>(data) {
            Ok(chunk) => {
                parsed_any = true;
                let choice = chunk.choices.into_iter().next();
                if let Some(reason) = choice.as_ref().and_then(|c| c.finish_reason.as_ref()) {
                    finish_reason = Some(reason.clone());
                }
                if let Some(t) = choice.and_then(|c| c.delta.content) {
                    if !t.is_empty() {
                        if first_content.is_none() {
                            first_content = Some(t0.elapsed().as_millis());
                            if debug {
                                eprintln!("[beek] 流式首字 {}ms", t0.elapsed().as_millis());
                            }
                        }
                        full.push_str(&t);
                        on_chunk(&t);
                    }
                }
            }
            Err(_) => return Err("翻译流包含无效或错误事件，请重试".into()),
        }
    }
    if debug {
        eprintln!(
            "[beek] 流式统计：原始行 {} 行 / {} 字节，首行 {}ms，首字 {}ms，总 {}ms",
            raw_lines,
            bytes,
            first_raw.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
            first_content.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
            t0.elapsed().as_millis()
        );
    }
    if finish_reason.as_deref().is_some_and(|r| r != "stop") {
        return Err("翻译未完整结束，请重试".into());
    }
    if !done && finish_reason.as_deref() != Some("stop") {
        return Err("翻译连接提前结束，请重试".into());
    }
    if !full.trim().is_empty() {
        return Ok(full.trim().to_string());
    }
    if parsed_any {
        Err("空响应".to_string())
    } else {
        Err("解析流式响应失败".into())
    }
}

/// 流式翻译（整句/整段）：只出译文。
pub fn translate_stream(text: &str, on_chunk: impl FnMut(&str)) -> Result<String, String> {
    TranslationRequest::prepare(text, false)?.stream(on_chunk)
}

/// 流式详解（单个词）：首行是「读音：…」，其余是词性/释义/例句/搭配。
pub fn explain_stream(text: &str, on_chunk: impl FnMut(&str)) -> Result<String, String> {
    TranslationRequest::prepare(text, true)?.stream(on_chunk)
}

/// 从详解正文里拆出读音行：返回 (读音, 去掉读音行后的正文)。
///
/// 模型被要求第一行写「读音：/…/」，但在流式过程中第一行可能还没收完——
/// 所以只有确认第一行后面已经换行时才算数，否则原样显示，避免半截内容闪一下。
/// 非流式翻译：保留给自检对比，UI 不再使用。
pub fn translate(text: &str) -> Result<String, String> {
    let request = TranslationRequest::prepare(text, false)?;
    if request.engine != Engine::Ark { return request.stream(|_| {}); }
    let resp = request.start(false)?;
    let parsed: Resp = resp.json().map_err(|e| format!("{}{}",beek::core::localization::tr("解析响应失败："),e))?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "空响应".to_string())
}

/// 非流式详解：保留给自检对比，UI 不再使用。
pub fn explain(text: &str) -> Result<String, String> {
    let request = TranslationRequest::prepare(text, true)?;
    if request.engine != Engine::Ark { return request.stream(|_| {}); }
    let resp = request.start(false)?;
    let parsed: Resp = resp.json().map_err(|e| format!("{}{}",beek::core::localization::tr("解析响应失败："),e))?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "空响应".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    // These integration tests deliberately change the process-wide cache policy.
    // Run them serially so the disable scenario cannot clear another test's result.
    static CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn fixture_request(prompt: String, endpoint: String, model: String, key: String) -> TranslationRequest {
        let mut request = TranslationRequest::new(prompt, endpoint, model, key);
        request.cache_settings = CacheSettings::default;
        request
    }

    fn with_server(bodies: Vec<(&'static str, &'static str)>, test: impl FnOnce(String)) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let expected = bodies.len();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + std::time::Duration::from_secs(5);
            let mut count = 0;
            for (status, body) in bodies {
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "missing HTTP request");
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                // macOS accepted sockets inherit O_NONBLOCK from the listener.
                socket.set_nonblocking(false).unwrap();
                socket.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
                let mut data = Vec::new();
                let mut buf = [0; 4096];
                loop {
                    let n = socket.read(&mut buf).unwrap();
                    assert_ne!(n, 0);
                    data.extend_from_slice(&buf[..n]);
                    if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&data[..end]).to_lowercase();
                        let length: usize = headers.lines().find_map(|l| l.strip_prefix("content-length: ")).unwrap_or("0").parse().unwrap();
                        if data.len() >= end + 4 + length { break; }
                    }
                }
                count += 1;
                write!(socket, "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            count
        });
        test(url);
        assert_eq!(worker.join().unwrap(), expected);
    }

    const COMPLETE: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"测试译文\"}}]}\n\ndata: [DONE]\n\n";
    const PARTIAL: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"半截\"}}]}\n\n";
    const LIMITED: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"半截\"},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n";
    const ERROR_AFTER_CONTENT: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"半截\"}}]}\n\ndata: {\"error\":\"fixture failure\"}\n\ndata: [DONE]\n\n";

    #[test]
    fn repeat_translation_reuses_completed_result_without_another_http_request() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        with_server(vec![("200 OK", COMPLETE)], |url| {
            let request = fixture_request(translate_prompt("Public fixture."), url, "fixture-model".into(), "fixture-key".into());
            let mut chunks = String::new();
            assert_eq!(request.stream(|c| chunks.push_str(c)).unwrap(), "测试译文");
            assert_eq!(chunks, "测试译文");
            assert_eq!(request.cached().as_deref(), Some("测试译文"));
            // The server has consumed its sole request; a second HTTP call would fail.
            assert_eq!(request.stream(|_| {}).unwrap(), "测试译文");
        });
    }

    #[test]
    fn switching_engines_isolates_cache_and_preserves_the_in_flight_snapshot() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        with_server(vec![("200 OK", "{\"translatedText\":\"本地译文\"}")], |url| {
            let mut cfg=crate::config::Config::default();
            cfg.translation_engine=Engine::LibreTranslate;
            cfg.set_provider("libretranslate",EngineConfig { base_url:Some(url),..Default::default() });
            let mut local=TranslationRequest::from_config(&cfg,"Public selection.",true).unwrap();
            local.cache_settings=CacheSettings::default;
            assert!(!local.is_dictionary());
            assert_eq!(local.prompt,"Public selection.");
            assert_eq!(local.stream(|_|{}).unwrap(),"本地译文");
            cfg.translation_engine=Engine::Azure;
            cfg.set_provider("azure",EngineConfig { api_key:"azure-fixture".into(),region:"eastasia".into(),..Default::default() });
            let mut azure=TranslationRequest::from_config(&cfg,"Public selection.",true).unwrap();
            azure.cache_settings=CacheSettings::default;
            assert_ne!(local.cache_key,azure.cache_key);
            assert!(azure.cached().is_none());
            assert_eq!(local.engine,Engine::LibreTranslate);
            assert_eq!(local.stream(|_|{}).unwrap(),"本地译文"); // no second HTTP request
            cfg.translation_engine=Engine::Unsupported;
            assert!(TranslationRequest::from_config(&cfg,"Public selection.",false).is_err());
        });
    }

    #[test]
    fn non_ai_business_errors_are_not_cached() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        with_server(vec![("200 OK","{\"error\":\"fixture-secret\"}"),("200 OK","{\"translatedText\":\"重试成功\"}")],|url|{
            let mut cfg=crate::config::Config::default();cfg.translation_engine=Engine::LibreTranslate;
            cfg.set_provider("libretranslate",EngineConfig { base_url:Some(url),..Default::default() });
            let mut request=TranslationRequest::from_config(&cfg,"Public retry selection.",false).unwrap();request.cache_settings=CacheSettings::default;
            assert!(request.stream(|_|{}).is_err());assert!(request.cached().is_none());
            assert_eq!(request.stream(|_|{}).unwrap(),"重试成功");
        });
    }

    #[test]
    fn every_provider_reuses_a_completed_result_without_second_http_request() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for (engine,response) in [
            (Engine::Ark,COMPLETE),
            (Engine::Azure,r#"[{"translations":[{"text":"测试译文"}]}]"#),
            (Engine::Baidu,r#"{"trans_result":[{"dst":"测试译文"}]}"#),
            (Engine::PublicBaidu,r#"{"translatedText":"测试译文"}"#),
            (Engine::Google,r#"{"data":{"translations":[{"translatedText":"测试译文"}]}}"#),
            (Engine::MyMemory,r#"{"responseStatus":200,"responseData":{"translatedText":"测试译文"}}"#),
            (Engine::LibreTranslate,r#"{"translatedText":"测试译文"}"#),
        ] {
            with_server(vec![("200 OK",response)],|url| {
                let mut cfg=crate::config::Config::default();cfg.translation_engine=engine;
                cfg.set_provider(engine.key(),EngineConfig {api_key:"fixture-key".into(),app_id:"fixture-app".into(),base_url:Some(url.clone()),..Default::default()});
                let mut first=TranslationRequest::from_config(&cfg,"Public cached sample.",false).unwrap();
                first.endpoint=url.clone();first.cache_settings=CacheSettings::default;
                assert!(first.cached().is_none());
                assert_eq!(first.stream(|_|{}).unwrap(),"测试译文","{engine:?}");
                // A separate production snapshot must resolve the same cache identity.
                let mut repeated=TranslationRequest::from_config(&cfg,"Public cached sample.",false).unwrap();
                repeated.endpoint=url;repeated.cache_settings=CacheSettings::default;
                assert_eq!(repeated.cached().as_deref(),Some("测试译文"),"{engine:?}");
                assert_eq!(repeated.stream(|_|{}).unwrap(),"测试译文","{engine:?}");
            });
        }
    }

    #[test]
    fn all_engines_isolate_target_languages_and_reuse_results_after_switching_back() {
        use crate::config::TargetLanguage;
        let _guard=CACHE_TEST_LOCK.lock().unwrap_or_else(|e|e.into_inner());
        for (engine,response) in [
            (Engine::Ark,COMPLETE),(Engine::Azure,r#"[{"translations":[{"text":"测试译文"}]}]"#),
            (Engine::Baidu,r#"{"trans_result":[{"dst":"测试译文"}]}"#),(Engine::PublicBaidu,r#"{"translatedText":"测试译文"}"#),
            (Engine::Google,r#"{"data":{"translations":[{"translatedText":"测试译文"}]}}"#),
            (Engine::MyMemory,r#"{"responseStatus":200,"responseData":{"translatedText":"测试译文"}}"#),
            (Engine::LibreTranslate,r#"{"translatedText":"测试译文"}"#),
        ] {
            with_server(vec![("200 OK",response),("200 OK",response)],|url|{
                let mut cfg=crate::config::Config::default();cfg.translation_engine=engine;
                cfg.translation_target=TargetLanguage::Japanese;
                cfg.set_provider(engine.key(),EngineConfig{api_key:"fixture-key".into(),app_id:"fixture-app".into(),base_url:Some(url.clone()),..Default::default()});
                let make=|cfg:&crate::config::Config|{let mut r=TranslationRequest::from_config(cfg,"Target isolation public fixture.",false).unwrap();r.endpoint=url.clone();r.cache_settings=CacheSettings::default;r};
                let ja=make(&cfg);assert!(ja.cached().is_none());ja.stream(|_|{}).unwrap();
                cfg.translation_target=TargetLanguage::English;
                let en=make(&cfg);assert_ne!(ja.cache_key,en.cache_key);assert!(en.cached().is_none());en.stream(|_|{}).unwrap();
                assert_eq!(ja.engine_config.target_language,TargetLanguage::Japanese);
                cfg.translation_target=TargetLanguage::Japanese;let back=make(&cfg);assert_eq!(ja.cache_key,back.cache_key);
                assert!(back.cached().is_some());back.stream(|_|{}).unwrap(); // A third HTTP request would fail.
            });
        }
    }

    #[test]
    fn disabling_cache_during_a_request_prevents_late_result_insertion() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use std::sync::atomic::{AtomicBool, Ordering};
        static DISABLED: AtomicBool = AtomicBool::new(false);
        fn settings() -> CacheSettings {
            if DISABLED.load(Ordering::SeqCst) {
                CacheSettings::from_input("0", "30").unwrap()
            } else { CacheSettings::default() }
        }
        with_server(vec![("200 OK", COMPLETE)], |url| {
            let mut request = fixture_request(translate_prompt("Public late result."), url, "fixture-model".into(), "fixture-key".into());
            request.cache_settings = settings;
            assert_eq!(request.stream(|_| DISABLED.store(true, Ordering::SeqCst)).unwrap(), "测试译文");
            assert!(request.cached().is_none());
        });
    }

    #[test]
    fn truncated_failed_and_limited_streams_are_not_cached_and_can_retry() {
        let _guard = CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for failure in [("200 OK", PARTIAL), ("200 OK", LIMITED), ("200 OK", ERROR_AFTER_CONTENT), ("200 OK", "data: {\"error\":\"fixture failure\"}\n\ndata: [DONE]\n\n"), ("200 OK", "data: [DONE]\n\n"), ("500 Error", "fixture failure")] {
            with_server(vec![failure, ("200 OK", COMPLETE)], |url| {
                let request = fixture_request(translate_prompt("Public retry."), url, "fixture-model".into(), "fixture-key".into());
                assert!(request.stream(|_| {}).is_err());
                assert!(request.cached().is_none());
                assert_eq!(request.stream(|_| {}).unwrap(), "测试译文");
            });
        }
    }

    #[test]
    fn cache_identity_separates_prompt_mode_endpoint_model_and_account() {
        let base = fixture_request(translate_prompt("word"), "url".into(), "model".into(), "key".into());
        for other in [
            fixture_request(explain_prompt("word"), "url".into(), "model".into(), "key".into()),
            fixture_request(translate_prompt("Word"), "url".into(), "model".into(), "key".into()),
            fixture_request(translate_prompt("word"), "other".into(), "model".into(), "key".into()),
            fixture_request(translate_prompt("word"), "url".into(), "other".into(), "key".into()),
            fixture_request(translate_prompt("word"), "url".into(), "model".into(), "other".into()),
        ] { assert_ne!(base.cache_key, other.cache_key); }
    }
}
