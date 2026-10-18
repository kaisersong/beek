use std::{
    io::{self, BufRead, Read},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};

const FRAME: usize = 64 * 1024;
const OUTPUT: usize = 256 * 1024;
pub struct Request {
    pub generation: u64,
    pub text: String,
    pub key: String,
    pub model: String,
    pub endpoint: String,
}
#[derive(Clone)]
pub enum Progress {
    Idle,
    Queued,
    Loading,
    Text(String),
    Done(Result<String, String>),
}
pub type Mailbox = Arc<Mutex<Progress>>;
struct Job {
    request: Request,
    mailbox: Mailbox,
    deadline: Instant,
    reported: Arc<AtomicBool>,
}
#[derive(Clone)]
struct Active {
    mailbox: Mailbox,
    deadline: Instant,
    reported: Arc<AtomicBool>,
}
#[derive(Default)]
struct State {
    pending: Option<Job>,
    stop: bool,
    active: Vec<Active>,
}
pub struct Pool {
    shared: Arc<(Mutex<State>, Condvar)>,
}
impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}
impl Pool {
    pub fn new() -> Self {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        // Exactly two real blocking HTTP workers. Logical cancellation never frees a
        // live HTTP slot; pending work is one replaceable latest request.
        for _ in 0..2 {
            let shared = shared.clone();
            std::thread::spawn(move || loop {
                let job = {
                    let (lock, wake) = &*shared;
                    let mut state = lock.lock().unwrap();
                    while state.pending.is_none() && !state.stop {
                        state = wake.wait(state).unwrap();
                    }
                    if state.stop {
                        return;
                    }
                    let job = state.pending.take().unwrap();
                    state.active.push(Active {
                        mailbox: job.mailbox.clone(),
                        deadline: job.deadline,
                        reported: job.reported.clone(),
                    });
                    job
                };
                {
                    let mut progress = job.mailbox.lock().unwrap();
                    if !job.reported.load(Ordering::Acquire) {
                        *progress = Progress::Loading;
                    }
                }
                let mut full = String::new();
                let mut at = Instant::now();
                let mut bytes = 0;
                let client = client();
                let result = client.as_ref().map_err(Clone::clone).and_then(|client| {
                    send(client, &job.request, job.deadline, |part| {
                        full.push_str(part);
                        bytes += part.len();
                        if at.elapsed() >= Duration::from_millis(30) || bytes >= 4096 {
                            let mut progress = job.mailbox.lock().unwrap();
                            if !job.reported.load(Ordering::Acquire) {
                                *progress = Progress::Text(full.clone());
                            }
                            at = Instant::now();
                            bytes = 0;
                        }
                    })
                });
                if job
                    .reported
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    *job.mailbox.lock().unwrap() =
                        Progress::Done(if Instant::now() > job.deadline {
                            Err("翻译请求超时，请重试".into())
                        } else {
                            result
                        });
                }
                // Runtime destruction can wait for an OS DNS call. Publish the
                // result first, but retain this real worker slot until it ends.
                drop(client);
                let (lock, _) = &*shared;
                lock.lock()
                    .unwrap()
                    .active
                    .retain(|a| !Arc::ptr_eq(&a.reported, &job.reported));
            });
        }
        let watcher = shared.clone();
        std::thread::spawn(move || loop {
            let expired = {
                let (lock, _) = &*watcher;
                let mut state = lock.lock().unwrap();
                if state.stop {
                    return;
                }
                let now = Instant::now();
                let mut expired = state
                    .active
                    .iter()
                    .filter(|a| a.deadline <= now)
                    .cloned()
                    .collect::<Vec<_>>();
                if state.pending.as_ref().is_some_and(|j| j.deadline <= now) {
                    let job = state.pending.take().unwrap();
                    expired.push(Active {
                        mailbox: job.mailbox,
                        deadline: job.deadline,
                        reported: job.reported,
                    });
                }
                expired
            };
            for job in expired {
                if job
                    .reported
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    *job.mailbox.lock().unwrap() =
                        Progress::Done(Err("翻译请求超时或等待槽位超时，请重试".into()));
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        });
        Self { shared }
    }
    pub fn submit(&self, request: Request) -> Mailbox {
        let mailbox = Arc::new(Mutex::new(Progress::Queued));
        let (lock, wake) = &*self.shared;
        let mut state = lock.lock().unwrap();
        if let Some(replaced) = state.pending.replace(Job {
            request,
            mailbox: mailbox.clone(),
            deadline: Instant::now() + Duration::from_secs(60),
            reported: Arc::new(AtomicBool::new(false)),
        }) {
            *replaced.mailbox.lock().unwrap() =
                Progress::Done(Err("请求已被较新的操作替换".into()));
        }
        wake.notify_one();
        mailbox
    }
    pub fn cancel_pending(&self) {
        let pending = { self.shared.0.lock().unwrap().pending.take() };
        if let Some(job) = pending {
            job.reported.store(true, Ordering::Release);
            *job.mailbox.lock().unwrap() = Progress::Done(Err("请求已取消".into()));
        }
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        let (lock, wake) = &*self.shared;
        let mut s = lock.lock().unwrap();
        s.stop = true;
        s.pending = None;
        wake.notify_all();
    }
}

pub use super::prompts::{is_single_word as is_word, split_pron};
pub fn prompt(text: &str) -> String {
    if is_word(text) {
        super::prompts::explain_prompt(text)
    } else {
        super::prompts::translate_prompt(text)
    }
}
fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法建立网络客户端".into())
}
fn send(
    client: &reqwest::blocking::Client,
    r: &Request,
    deadline: Instant,
    on_chunk: impl FnMut(&str),
) -> Result<String, String> {
    if r.text.trim().is_empty() || r.text.chars().count() > super::protocol::MAX_CHARS {
        return Err("选区为空或超出长度限制".into());
    }
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or("翻译请求已超时")?;
    // A request timeout installs the async body's total deadline too. The
    // blocking client's default alone only bounds each individual operation.
    let response=client.post(&r.endpoint).timeout(remaining).bearer_auth(&r.key).json(&serde_json::json!({"model":r.model,"messages":[{"role":"user","content":prompt(&r.text)}],"stream":true,"thinking":{"type":"disabled"}})).send().map_err(|_|"翻译请求失败或超时，请检查网络和 API 设置")?;
    if !response.status().is_success() {
        return Err(format!(
            "翻译服务返回 HTTP {}，请检查 API 设置或稍后重试",
            response.status().as_u16()
        ));
    }
    parse(io::BufReader::new(response), on_chunk)
}
pub fn parse(mut reader: impl BufRead, mut on_chunk: impl FnMut(&str)) -> Result<String, String> {
    let mut full = String::new();
    let mut frame = Vec::new();
    let mut total = 0usize;
    let mut finished = false;
    loop {
        let mut line = Vec::new();
        let n = (&mut reader)
            .take((FRAME + 3) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| "读取翻译流失败或超时")?;
        if n > FRAME + 2 {
            return Err("翻译流单行超过64KiB限制".into());
        }
        total += n;
        if total > 4 * 1024 * 1024 {
            return Err("翻译流超过传输上限".into());
        }
        if n == 0 {
            if !frame.is_empty() {
                consume(&frame, &mut full, &mut finished, &mut on_chunk)?;
            }
            break;
        }
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        if line.len() > FRAME {
            return Err("翻译流单行超过64KiB限制".into());
        }
        if line.is_empty() {
            if !frame.is_empty() {
                if consume(&frame, &mut full, &mut finished, &mut on_chunk)? {
                    break;
                }
                frame.clear();
            }
        } else if let Some(data) = line.strip_prefix(b"data:") {
            if frame.len() + data.len() + 1 > FRAME {
                return Err("翻译事件超过64KiB限制".into());
            }
            if !frame.is_empty() {
                frame.push(b'\n');
            }
            frame.extend_from_slice(data.strip_prefix(b" ").unwrap_or(data));
        }
    }
    if !finished {
        return Err("翻译流提前中断，请重试".into());
    }
    if full.trim().is_empty() {
        return Err("翻译服务返回空响应".into());
    }
    Ok(full.trim().into())
}
fn consume(
    frame: &[u8],
    full: &mut String,
    finished: &mut bool,
    chunk: &mut impl FnMut(&str),
) -> Result<bool, String> {
    if frame == b"[DONE]" {
        *finished = true;
        return Ok(true);
    }
    let data: serde_json::Value = serde_json::from_slice(frame).map_err(|_| "翻译流格式无效")?;
    if data.get("error").is_some() {
        return Err("翻译服务返回错误，请检查 API 设置".into());
    }
    if let Some(choice) = data.get("choices").and_then(|c| c.get(0)) {
        if let Some(t) = choice.pointer("/delta/content").and_then(|v| v.as_str()) {
            if full.len() + t.len() > OUTPUT {
                return Err("译文超过256KiB限制".into());
            }
            full.push_str(t);
            chunk(t);
        }
        if choice.get("finish_reason").is_some_and(|f| !f.is_null()) {
            *finished = true;
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_utf8_sse_and_finish_are_preserved() {
        let s =
            "data: {\"choices\":[{\"delta\":{\"content\":\"中文😀\"}}]}\r\n\r\ndata: [DONE]\n\n";
        let reader = io::BufReader::with_capacity(1, s.as_bytes());
        let mut seen = String::new();
        assert_eq!(parse(reader, |t| seen.push_str(t)).unwrap(), "中文😀");
        assert_eq!(seen, "中文😀");
    }
    #[test]
    fn partial_content_does_not_turn_disconnect_into_success() {
        assert!(parse(
            io::Cursor::new(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n"),
            |_| {}
        )
        .is_err());
    }
    #[test]
    fn huge_line_and_aggregate_output_are_bounded() {
        assert!(parse(io::Cursor::new(vec![b'x'; FRAME + 1]), |_| {}).is_err());
        let piece = "a".repeat(32 * 1024);
        let event =
            format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{piece}\"}}}}]}}\n\n");
        let data = event.repeat(9) + "data: [DONE]\n\n";
        assert!(parse(io::Cursor::new(data), |_| {}).is_err());
    }
    #[test]
    fn empty_done_is_not_a_translation() {
        assert!(parse(io::Cursor::new(b"data: [DONE]\n\n"), |_| {}).is_err());
    }
    #[test]
    fn line_boundary_excludes_crlf_and_rejects_one_extra_content_byte() {
        for ending in ["\n", "\r\n"] {
            let comment = format!(":{}{}", "a".repeat(FRAME - 1), ending);
            let valid = comment
                + "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n";
            assert_eq!(parse(io::Cursor::new(valid), |_| {}).unwrap(), "ok");
        }
        assert!(parse(io::Cursor::new(format!(":{}\n", "a".repeat(FRAME))), |_| {}).is_err());
    }
}
