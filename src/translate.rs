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
struct StreamChoice { delta: Delta }
#[derive(Deserialize)]
struct StreamChunk { choices: Vec<StreamChoice> }

// 非流式结构（自检对比用）
#[derive(Deserialize)]
struct BlockChoice { message: Msg }
#[derive(Deserialize)]
struct Resp { choices: Vec<BlockChoice> }

/// 选中的是不是「单个词」：没有空白、不太长、至少含一个字母（拉丁字母或汉字）。
/// 选中单个词时给词典式详解（含读音），而不是只给一句译文。
pub fn is_single_word(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.chars().count() > 24 {
        return false;
    }
    if t.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    t.chars().any(|c| c.is_alphabetic())
}

/// 读音行的固定前缀：模型必须用它开头，方便客户端把读音单独提出来做成标题行
const PRON_PREFIX: &str = "读音：";

fn translate_prompt(text: &str) -> String {
    format!(
        "将以下文本翻译成中文。要求：简洁准确，只输出译文不要解释。\
        AI 语境中的 Agent 必须译为\"智能体\"，不能译为\"代理人\"。\n\n{}",
        text
    )
}

fn explain_prompt(text: &str) -> String {
    format!(
        "你是一本词典。请针对「{}」这个词给出详解，并给出它的读音。用中文讲解，\
        严格按下面的格式输出，不要任何前言、标题、总结或多余空行：\n\
        第一行：{}<读音>（英文词给英式/美式国际音标，例如 /ˈeɪdʒənt/；中文词给拼音）\n\
        之后每行以「- 」开头，依次给出：词性与中文释义（多个义项就多行）、\
        例句（英文原句 + 中文翻译）、常见搭配或短语；必要时再加一行用法辨析。\n\
        要求具体、不要套话。",
        text, PRON_PREFIX
    )
}

fn build_body(prompt: String, stream: bool) -> Req {
    Req {
        model: model(),
        messages: vec![Msg { role: "user".into(), content: prompt }],
        stream,
        // 关掉思维链：这个模型默认会先“想”几秒，用户等的是译文而不是推理过程
        thinking: Thinking { kind: "disabled" },
    }
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())
}

/// 生效的模型：配置里填了就用配置的，否则用内置默认
fn model() -> String {
    crate::config::load()
        .provider(PROVIDER)
        .model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

/// 生效的 API Key：配置文件优先，环境变量兜底。
///
/// 两种情况都给：图形界面配的（菜单栏「设置 API Key…」→ config.json）
/// 和命令行/LaunchAgent 配的（BEEK_ARK_KEY），谁方便用谁。
fn api_key() -> Result<String, String> {
    let from_config = crate::config::load().provider(PROVIDER).api_key;
    if !from_config.trim().is_empty() {
        return Ok(from_config.trim().to_string());
    }
    std::env::var("BEEK_ARK_KEY")
        .map(|k| k.trim().to_string())
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| {
            "未配置 API Key：点菜单栏 BeeK →「设置 API Key…」，或设置环境变量 BEEK_ARK_KEY".to_string()
        })
}

/// 生效的端点：配置里填了 base_url 就用它（可接任意 OpenAI 兼容服务），否则用方舟默认地址
fn endpoint() -> String {
    crate::config::load()
        .provider(PROVIDER)
        .base_url
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| ARK_URL.to_string())
}

fn start(prompt: String, stream: bool) -> Result<reqwest::blocking::Response, String> {
    let key = api_key()?;
    let resp = client()?
        .post(endpoint())
        .header("Authorization", format!("Bearer {}", key))
        .json(&build_body(prompt, stream))
        .send()
        .map_err(|e| format!("请求失败：{}", e))?;
    if !resp.status().is_success() {
        let code = resp.status();
        let body: String = resp.text().unwrap_or_default().chars().take(200).collect();
        return Err(format!("HTTP {} {}", code, body));
    }
    Ok(resp)
}

/// 流式请求：每收到一段正文增量就回调一次 `on_chunk`，返回完整正文。
fn stream(prompt: String, mut on_chunk: impl FnMut(&str)) -> Result<String, String> {
    let debug = std::env::var("BEEK_DEBUG").is_ok();
    let t0 = std::time::Instant::now();
    let resp = start(prompt, true)?;
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
    let mut first_line = String::new();
    let mut parsed_any = false;
    let mut raw_lines = 0usize;
    let mut bytes = 0usize;
    let mut first_raw: Option<u128> = None;
    let mut first_content: Option<u128> = None;
    for line in std::io::BufRead::lines(std::io::BufReader::new(resp)) {
        let line = line.map_err(|e| format!("读取流失败：{}", e))?;
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
            break;
        }
        match serde_json::from_str::<StreamChunk>(data) {
            Ok(chunk) => {
                parsed_any = true;
                if let Some(t) = chunk.choices.into_iter().next().and_then(|c| c.delta.content) {
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
            Err(e) => {
                if !parsed_any {
                    first_line = format!("{}（原始片段：{}）", e, data.chars().take(200).collect::<String>());
                }
            }
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
    if !full.trim().is_empty() {
        return Ok(full.trim().to_string());
    }
    if parsed_any {
        Err("空响应".to_string())
    } else {
        Err(format!("解析流式响应失败：{}", first_line))
    }
}

/// 流式翻译（整句/整段）：只出译文。
pub fn translate_stream(text: &str, on_chunk: impl FnMut(&str)) -> Result<String, String> {
    stream(translate_prompt(text), on_chunk)
}

/// 流式详解（单个词）：首行是「读音：…」，其余是词性/释义/例句/搭配。
pub fn explain_stream(text: &str, on_chunk: impl FnMut(&str)) -> Result<String, String> {
    stream(explain_prompt(text), on_chunk)
}

/// 从详解正文里拆出读音行：返回 (读音, 去掉读音行后的正文)。
///
/// 模型被要求第一行写「读音：/…/」，但在流式过程中第一行可能还没收完——
/// 所以只有确认第一行后面已经换行时才算数，否则原样显示，避免半截内容闪一下。
pub fn split_pron(raw: &str) -> (Option<String>, String) {
    let Some((first, rest)) = raw.split_once('\n') else {
        return (None, raw.to_string());
    };
    let head = first.trim();
    let pron = head
        .strip_prefix(PRON_PREFIX)
        .or_else(|| head.strip_prefix("读音:"))
        .or_else(|| head.strip_prefix("音标："))
        .or_else(|| head.strip_prefix("音标:"))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    match pron {
        Some(p) => (Some(p), rest.trim_start().to_string()),
        None => (None, raw.to_string()),
    }
}

/// 非流式翻译：保留给自检对比，UI 不再使用。
pub fn translate(text: &str) -> Result<String, String> {
    let resp = start(translate_prompt(text), false)?;
    let parsed: Resp = resp.json().map_err(|e| format!("解析响应失败：{}", e))?;
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
    let resp = start(explain_prompt(text), false)?;
    let parsed: Resp = resp.json().map_err(|e| format!("解析响应失败：{}", e))?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "空响应".to_string())
}
