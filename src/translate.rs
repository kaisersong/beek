// BeeK 翻译模块 — 豆包 Ark API
use serde::{Deserialize, Serialize};

const ARK_URL: &str = "https://ark.cn-beijing.volces.com/api/v3/chat/completions";
const MODEL: &str = "doubao-seed-2-0-mini-260428";

#[derive(Serialize, Deserialize)]
struct Msg { role: String, content: String }
#[derive(Serialize)]
struct Req { model: String, messages: Vec<Msg> }
#[derive(Deserialize)]
struct Choice { message: Msg }
#[derive(Deserialize)]
struct Resp { choices: Vec<Choice> }

/// 翻译文本。key 从环境变量 BEEK_ARK_KEY 读取。
pub fn translate(text: &str) -> Result<String, String> {
    let key = std::env::var("BEEK_ARK_KEY").map_err(|_| "未设置 BEEK_ARK_KEY".to_string())?;
    let prompt = format!(
        "将以下文本翻译成中文。要求：简洁准确，只输出译文不要解释。\
        AI 语境中的 Agent 必须译为\"智能体\"，不能译为\"代理人\"。\n\n{}",
        text
    );
    let req = Req {
        model: MODEL.to_string(),
        messages: vec![Msg { role: "user".into(), content: prompt }],
    };
    let client = reqwest::blocking::Client::new();
    let resp: Resp = client
        .post(ARK_URL)
        .header("Authorization", format!("Bearer {}", key))
        .json(&req)
        .send()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    resp.choices.into_iter().next()
        .map(|c| c.message.content.trim().to_string())
        .ok_or_else(|| "空响应".to_string())
}
