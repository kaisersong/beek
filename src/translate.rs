// BeeK 翻译模块 — 豆包 Ark API（流式 + 关闭思维链）
//
// 实测（短句，本机）：
//   非流式 + 思维链开启  3.12s   ← 旧行为，弹窗要等这么久
//   非流式 + 关闭思维链  1.09s
//   流式   + 关闭思维链  0.62s 见到首字
// 因此这里固定关闭 thinking 并走流式，让弹窗能立刻显示译文增量。
use serde::{Deserialize, Serialize};

const ARK_URL: &str = "https://ark.cn-beijing.volces.com/api/v3/chat/completions";
const MODEL: &str = "doubao-seed-2-0-mini-260428";
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

fn build_body(text: &str, stream: bool) -> Req {
    let prompt = format!(
        "将以下文本翻译成中文。要求：简洁准确，只输出译文不要解释。\
        AI 语境中的 Agent 必须译为\"智能体\"，不能译为\"代理人\"。\n\n{}",
        text
    );
    Req {
        model: MODEL.to_string(),
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

fn api_key() -> Result<String, String> {
    std::env::var("BEEK_ARK_KEY").map_err(|_| "未设置 BEEK_ARK_KEY".to_string())
}

fn start(text: &str, stream: bool) -> Result<reqwest::blocking::Response, String> {
    let key = api_key()?;
    let resp = client()?
        .post(ARK_URL)
        .header("Authorization", format!("Bearer {}", key))
        .json(&build_body(text, stream))
        .send()
        .map_err(|e| format!("请求失败：{}", e))?;
    if !resp.status().is_success() {
        let code = resp.status();
        let body: String = resp.text().unwrap_or_default().chars().take(200).collect();
        return Err(format!("HTTP {} {}", code, body));
    }
    Ok(resp)
}

/// 流式翻译：每收到一段正文增量就回调一次 `on_chunk`，返回完整译文。
pub fn translate_stream(text: &str, mut on_chunk: impl FnMut(&str)) -> Result<String, String> {
    let debug = std::env::var("BEEK_DEBUG").is_ok();
    let t0 = std::time::Instant::now();
    let resp = start(text, true)?;
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

/// 非流式翻译：保留给自检对比，UI 不再使用。
pub fn translate(text: &str) -> Result<String, String> {
    let resp = start(text, false)?;
    let parsed: Resp = resp.json().map_err(|e| format!("解析响应失败：{}", e))?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "空响应".to_string())
}
