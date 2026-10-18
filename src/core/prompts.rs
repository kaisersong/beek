//! Prompts and text rules shared verbatim with the macOS translator.
pub const DEFAULT_MODEL: &str = "doubao-seed-2-0-mini-260428";
pub const DEFAULT_ENDPOINT: &str = "https://ark.cn-beijing.volces.com/api/v3/chat/completions";
pub fn is_url(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("http://") || t.starts_with("https://") || t.starts_with("www.")
}
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

pub fn translate_prompt(text: &str) -> String {
    format!(
        "将以下文本翻译成中文。要求：简洁准确，只输出译文不要解释。\
        AI 语境中的 Agent 必须译为\"智能体\"，不能译为\"代理人\"。\n\n{}",
        text
    )
}

pub fn explain_prompt(text: &str) -> String {
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
