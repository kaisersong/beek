# Windows implementation review snapshot

Only this subsystem is in scope. Full G5 is NOT passed. G0-D allows internal development. Do not treat missing external signing/multi-monitor/soak evidence as a code bug, and do not approve public release. Review adversarially for concrete reproducible bugs, not generic concerns. Read permission includes continuing this same attachment if truncated.

## src/core/stream.rs
```
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
            .take((FRAME + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| "读取翻译流失败或超时")?;
        if n > FRAME {
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
}

```

## src/core/release.rs
```
//! Exact stable Windows asset selection; returned URLs are never executed.
use serde::Deserialize;
#[derive(Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}
#[derive(Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}
pub fn version(value: &str) -> Option<[u32; 3]> {
    let v = value.strip_prefix('v').unwrap_or(value);
    let items = v.split('.').collect::<Vec<_>>();
    if items.len() != 3 {
        return None;
    }
    let mut out = [0; 3];
    for (i, v) in items.iter().enumerate() {
        if v.is_empty() || !v.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        out[i] = v.parse().ok()?;
    }
    Some(out)
}
pub fn windows_update(release: &Release, current: &str) -> Result<Option<String>, String> {
    if release.draft || release.prerelease {
        return Err("该版本不是稳定发布".into());
    }
    let latest = version(&release.tag_name).ok_or("发布版本格式无效")?;
    let current = version(current).ok_or("当前版本格式无效")?;
    if latest <= current {
        return Ok(None);
    }
    let number = format!("{}.{}.{}", latest[0], latest[1], latest[2]);
    for suffix in ["windows-x64.zip", "windows-x64-setup.exe"] {
        let name = format!("BeeK-v{number}-{suffix}");
        let url = format!("https://github.com/kaisersong/beek/releases/download/v{number}/{name}");
        if !release
            .assets
            .iter()
            .any(|a| a.name == name && a.browser_download_url == url)
        {
            return Err(format!("版本 {number} 暂无完整 Windows x64 安装包"));
        }
    }
    Ok(Some(number))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_platform_channel_and_urls() {
        let mut r = Release {
            tag_name: "v1.0.15".into(),
            draft: false,
            prerelease: false,
            assets: vec![],
        };
        assert!(windows_update(&r, "1.0.14").is_err());
        for suffix in ["windows-x64.zip", "windows-x64-setup.exe"] {
            let name = format!("BeeK-v1.0.15-{suffix}");
            r.assets.push(Asset {
                browser_download_url: format!(
                    "https://github.com/kaisersong/beek/releases/download/v1.0.15/{name}"
                ),
                name,
            });
        }
        assert_eq!(windows_update(&r, "1.0.14").unwrap(), Some("1.0.15".into()));
        r.assets[1].browser_download_url = "https://other.invalid/setup.exe".into();
        assert!(windows_update(&r, "1.0.14").is_err());
        r.prerelease = true;
        assert!(windows_update(&r, "1.0.14").is_err());
        assert!(version("1.0.15-beta").is_none());
    }
}

```

## src/platform/windows/integration.rs
```
use super::util;
use windows::{
    core::w,
    Win32::{System::Registry::*, UI::Shell::*, UI::WindowsAndMessaging::SW_SHOWNORMAL},
};
type UpdateWorker = (
    std::sync::mpsc::SyncSender<()>,
    std::sync::Mutex<std::sync::mpsc::Receiver<String>>,
);
static UPDATES: std::sync::OnceLock<UpdateWorker> = std::sync::OnceLock::new();
pub fn open_updates() -> bool {
    let (tx, _) = UPDATES.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let (out, result) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                let status = check_update();
                if out.send(status).is_err() {
                    break;
                }
            }
        });
        (tx, std::sync::Mutex::new(result))
    });
    tx.try_send(()).is_ok()
}
fn check_update() -> String {
    use std::{io::Read, time::Duration};
    let result = (|| -> Result<Option<String>, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "无法初始化更新检查")?;
        let mut response = client
            .get("https://api.github.com/repos/kaisersong/beek/releases/latest")
            .header("User-Agent", concat!("BeeK/", env!("CARGO_PKG_VERSION")))
            .send()
            .map_err(|_| "更新检查失败，请稍后重试")?;
        if !response.status().is_success() {
            return Err(format!(
                "更新检查失败（HTTP {}）",
                response.status().as_u16()
            ));
        }
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "更新响应读取失败")?;
        if bytes.len() > 1024 * 1024 {
            return Err("更新响应超出限制".into());
        }
        let release = serde_json::from_slice(&bytes).map_err(|_| "更新信息格式无效")?;
        crate::core::release::windows_update(&release, env!("CARGO_PKG_VERSION"))
    })();
    match result {
        Ok(None) => "当前已是最新 Windows 版本".into(),
        Ok(Some(version)) => {
            unsafe {
                let _ = ShellExecuteW(
                    None,
                    w!("open"),
                    w!("https://github.com/kaisersong/beek/releases"),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
            }
            format!("发现 {version}，已打开固定发布页；请下载并检查签名后安装")
        }
        Err(error) => error,
    }
}
pub fn update_result() -> Option<String> {
    UPDATES
        .get()
        .and_then(|(_, rx)| rx.lock().ok()?.try_recv().ok())
}
pub fn autostart(enabled: bool) -> Result<(), String> {
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()
        .map_err(|_| "配置已保存，但无法修改登录启动项")?;
        let result = if enabled {
            let path = std::env::current_exe().map_err(|_| "程序路径不可用")?;
            let value = util::wide(format!("\"{}\" --autostart", path.display()));
            let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
            RegSetValueExW(key, w!("BeeK"), None, REG_SZ, Some(bytes)).ok()
        } else {
            let r = RegDeleteValueW(key, w!("BeeK"));
            if r == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                r.ok()
            }
        };
        let _ = RegCloseKey(key);
        result.map_err(|_| "配置已保存，但登录启动项修改失败".into())
    }
}

```

## src/platform/windows/speech.rs
```
use std::sync::{mpsc, OnceLock};
static INPUT: OnceLock<mpsc::SyncSender<String>> = OnceLock::new();
pub fn speak(text: String) {
    if text.chars().count() > 16_384 {
        return;
    }
    let tx = INPUT.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<String>(1);
        std::thread::spawn(move || unsafe {
            use windows::Win32::{Media::Speech::*, System::Com::*};
            if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
                return;
            }
            if let Ok(voice) = CoCreateInstance::<_, ISpVoice>(&SpVoice, None, CLSCTX_INPROC_SERVER)
            {
                while let Ok(text) = rx.recv() {
                    let words = super::util::wide(text);
                    let _ = voice.Speak(
                        windows::core::PCWSTR(words.as_ptr()),
                        SPF_DEFAULT.0 as u32,
                        None,
                    );
                }
            }
            CoUninitialize();
        });
        tx
    });
    let _ = tx.try_send(text);
}

```

## scripts/windows/package.ps1
```
param([string]$TargetDirectory='',[string]$OutputDirectory='',[Parameter(Mandatory=$true)][string]$Iscc,[switch]$Formal,[string]$PublisherThumbprint='', [string]$SignTool='',[string]$TimestampUrl='http://timestamp.digicert.com')
$ErrorActionPreference='Stop'; Set-StrictMode -Version Latest
$PublisherThumbprint=$PublisherThumbprint.ToUpperInvariant()
$repo=(Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if(-not $TargetDirectory){$TargetDirectory=Join-Path $repo 'target\release'}
if(-not $OutputDirectory){$OutputDirectory=Join-Path $repo 'dist\windows'}
$version=([regex]::Match([IO.File]::ReadAllText((Join-Path $repo 'Cargo.toml')),'(?m)^version = "([^"]+)"')).Groups[1].Value
if($version -notmatch '^\d+\.\d+\.\d+$'){throw 'Invalid Cargo version'}
$exe=Join-Path $TargetDirectory 'beek.exe';$helper=Join-Path $TargetDirectory 'beek-selection-host.exe'
function Capabilities([string]$Path){$p=[Diagnostics.Process]::new();$p.StartInfo.FileName=(Resolve-Path $Path).Path;$p.StartInfo.Arguments='--capabilities';$p.StartInfo.UseShellExecute=$false;$p.StartInfo.RedirectStandardOutput=$true;$p.StartInfo.RedirectStandardError=$true;$p.StartInfo.CreateNoWindow=$true;$null=$p.Start();$json=$p.StandardOutput.ReadToEnd();$p.WaitForExit();if($p.ExitCode -ne 0){throw 'Capability command failed'};$p.Dispose();$json|ConvertFrom-Json}
$main=Capabilities $exe;$hostInfo=Capabilities $helper
if($main.build_id -ne $hostInfo.build_id -or $main.protocol -ne $hostInfo.protocol -or $main.binary -ne 'main' -or $hostInfo.binary -ne 'selection-host'){throw 'Main/helper pair mismatch'}
foreach($info in @($main,$hostInfo)){if($info.os -ne 'windows' -or $info.arch -ne 'x86_64' -or $info.version -ne $version -or $info.debug_assertions -or $info.test_fixtures){throw 'Only matching non-fixture release x64 builds can be packaged'}}
if($Formal -and (-not $PublisherThumbprint -or -not $SignTool)){throw 'Formal packages require explicit signer thumbprint and signtool'}
if(Test-Path $OutputDirectory){throw 'Use a fresh output directory; do not mix package generations'}
$null=New-Item -ItemType Directory $OutputDirectory -Force;$OutputDirectory=(Resolve-Path $OutputDirectory).Path
$payload=Join-Path $OutputDirectory 'payload';$null=New-Item -ItemType Directory $payload
Copy-Item $exe,$helper $payload
Copy-Item (Join-Path $repo 'LICENSE*') $payload
Copy-Item (Join-Path $repo 'resources\GPUI-*') $payload
Copy-Item (Join-Path $repo 'packaging\windows\README-WINDOWS.txt') $payload
# Source license inventory is generated from the exact locked dependency graph.
$licenseArgs=@((Join-Path $PSScriptRoot 'licenses.py'),'--out',(Join-Path $payload 'THIRD-PARTY-NOTICES.txt'));if($Formal){$licenseArgs+='--strict'};& python @licenseArgs
if($LASTEXITCODE -ne 0){throw 'Dependency license inventory failed'}
function Sign([string]$Path){
 if($Formal){& $SignTool sign /sha1 $PublisherThumbprint /fd SHA256 /tr $TimestampUrl /td SHA256 $Path;if($LASTEXITCODE -ne 0){throw 'Authenticode signing failed'}
 $signature=Get-AuthenticodeSignature $Path;if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $PublisherThumbprint){throw 'Signature publisher verification failed'}}
}
Sign (Join-Path $payload 'beek.exe');Sign (Join-Path $payload 'beek-selection-host.exe')
$files=@(Get-ChildItem $payload -File|Sort-Object Name|ForEach-Object {@{name=$_.Name;sha256=(Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower();bytes=$_.Length}})
$manifest=@{schema=1;version=$version;build_id=$main.build_id;protocol=$main.protocol;mode='portable';distribution=$(if($Formal){'formal-signed'}else{'internal-preview-unsigned'});files=$files}
[IO.File]::WriteAllText((Join-Path $payload 'manifest.json'),($manifest|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
$marker=Join-Path $repo 'packaging\windows\installed.marker'
$signArgs=@();if($Formal){if($PublisherThumbprint -notmatch '^[a-fA-F0-9]{40}$'){throw 'Invalid certificate thumbprint'};$signArgs=@('/DFormalSigning=1',('/Sbeek="'+$SignTool+'" sign /sha1 '+$PublisherThumbprint+' /fd SHA256 /tr '+$TimestampUrl+' /td SHA256 $f'))}
& $Iscc @signArgs "/DAppVersion=$version" "/DBuildId=$($main.build_id)" "/DPayloadDir=$payload" "/DOutputPath=$OutputDirectory" "/DMarkerPath=$marker" (Join-Path $repo 'packaging\windows\beek.iss')
if($LASTEXITCODE -ne 0){throw 'Installer compilation failed'}
$setup=Join-Path $OutputDirectory "BeeK-v$version-windows-x64-setup.exe";Sign $setup
if($Formal){$records=@{};foreach($file in @((Join-Path $payload 'beek.exe'),(Join-Path $payload 'beek-selection-host.exe'),$setup)){$signature=Get-AuthenticodeSignature $file;$records[[IO.Path]::GetFileName($file)]=@{status=$signature.Status.ToString();thumbprint=$signature.SignerCertificate.Thumbprint;sha256=(Get-FileHash $file -Algorithm SHA256).Hash.ToLower()}};@{schema=1;expected_thumbprint=$PublisherThumbprint;files=$records;signed_uninstaller=$true}|ConvertTo-Json -Depth 6|ForEach-Object {[IO.File]::WriteAllText((Join-Path $OutputDirectory 'windows-signatures.json'),$_,[Text.UTF8Encoding]::new($false))}}
$zip=Join-Path $OutputDirectory "BeeK-v$version-windows-x64.zip"
Compress-Archive -Path (Join-Path $payload '*') -DestinationPath $zip
foreach($file in @($zip,$setup)){[IO.File]::WriteAllText(($file+'.sha256'),((Get-FileHash $file -Algorithm SHA256).Hash.ToLower()+'  '+[IO.Path]::GetFileName($file)+"`n"),[Text.UTF8Encoding]::new($false))}
Write-Host "Created $($manifest.distribution) packages in $OutputDirectory. Packaging is not G5 acceptance."

```

## scripts/windows/release_assets.py
```
#!/usr/bin/env python3
"""Fail closed before publishing a stable, complete two-platform release."""
import argparse,hashlib,json,re,zipfile
from pathlib import Path

def validate(folder:Path,version:str,formal=True):
 errors=[]
 if not re.fullmatch(r'\d+\.\d+\.\d+',version):return ['invalid release version']
 names=[f'BeeK-v{version}{suffix}' for suffix in ('.zip','.dmg','-windows-x64.zip','-windows-x64-setup.exe')]
 for name in names:
  file=folder/name;checksum=folder/(name+'.sha256')
  if not file.is_file() or not checksum.is_file():errors.append('missing required asset/checksum: '+name);continue
  expected=hashlib.sha256(file.read_bytes()).hexdigest()
  if checksum.read_text().strip()!=expected+'  '+name:errors.append('asset checksum mismatch: '+name)
 winzip=folder/names[2]
 try:
  with zipfile.ZipFile(winzip) as archive:
   members=archive.namelist()
   if any(n.startswith(('/','\\')) or '..' in Path(n.replace('\\','/')).parts for n in members):errors.append('unsafe archive path')
   manifest=json.loads(archive.read('manifest.json'))
   if manifest.get('version')!=version or manifest.get('mode')!='portable':errors.append('wrong Windows manifest version/mode')
   if formal and manifest.get('distribution')!='formal-signed':errors.append('unsigned preview cannot be published')
   listed={entry['name'] for entry in manifest['files']}
   if not {'beek.exe','beek-selection-host.exe'}.issubset(listed):errors.append('incomplete Windows executable pair')
   if set(members)!=listed|{'manifest.json'}:errors.append('unlisted or missing payload files')
   for entry in manifest['files']:
    if '/' in entry['name'] or '\\' in entry['name']:errors.append('nested payload path');continue
    if hashlib.sha256(archive.read(entry['name'])).hexdigest()!=entry['sha256']:errors.append('payload hash mismatch: '+entry['name'])
 except (OSError,ValueError,KeyError,zipfile.BadZipFile):errors.append('invalid Windows package manifest')
 if formal:
  try:
   report=json.loads((folder/'windows-signatures.json').read_text())
   for name in ('beek.exe','beek-selection-host.exe',names[3]):
    item=report['files'][name]
    if item['status']!='Valid' or item['thumbprint']!=report['expected_thumbprint']:errors.append('invalid Windows signature: '+name)
    if name==names[3]:actual=hashlib.sha256((folder/name).read_bytes()).hexdigest()
    else:
     with zipfile.ZipFile(winzip) as archive:actual=hashlib.sha256(archive.read(name)).hexdigest()
    if item['sha256']!=actual:errors.append('signature attestation asset mismatch: '+name)
   if not report.get('signed_uninstaller'):errors.append('unsigned uninstaller')
  except (OSError,ValueError,KeyError,zipfile.BadZipFile):errors.append('missing or invalid Windows signature attestation')
 return errors

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--directory',type=Path,required=True);p.add_argument('--version',required=True);p.add_argument('--internal-preview',action='store_true');a=p.parse_args();errors=validate(a.directory,a.version,not a.internal_preview)
 for error in errors:print('NOT PASS: '+error)
 return 1 if errors else 0
if __name__=='__main__':raise SystemExit(main())

```

## packaging/windows/beek.iss
```
#ifndef AppVersion
  #error AppVersion required
#endif
#ifndef BuildId
  #error BuildId required
#endif
#ifndef PayloadDir
  #error PayloadDir required
#endif
#ifndef OutputPath
  #error OutputPath required
#endif
#define AppGuid "{8E674477-64D4-4AF7-B0E6-C64A5A6748F3}"
#define PayloadName AppVersion + "-" + BuildId
[Setup]
AppId={{8E674477-64D4-4AF7-B0E6-C64A5A6748F3}
AppName=BeeK
AppVersion={#AppVersion}
AppPublisher=BeeK
AppPublisherURL=https://github.com/kaisersong/beek
DefaultDirName={localappdata}\Programs\BeeK
DefaultGroupName=BeeK
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\versions\{#PayloadName}\beek.exe
OutputDir={#OutputPath}
OutputBaseFilename=BeeK-v{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
AppMutex=Local\BeeK-Running
SetupLogging=yes
#ifdef FormalSigning
SignedUninstaller=yes
SignTool=beek
#else
SignedUninstaller=no
#endif
WizardStyle=modern
[Files]
Source: "{#MarkerPath}"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\beek.exe"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\beek-selection-host.exe"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\LICENSE*"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\GPUI-*"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\manifest.json"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
Source: "{#PayloadDir}\README-WINDOWS.txt"; DestDir: "{app}\versions\{#PayloadName}"; Flags: ignoreversion
[Icons]
Name: "{autoprograms}\BeeK"; Filename: "{app}\versions\{#PayloadName}\beek.exe"
[Run]
Filename: "{app}\versions\{#PayloadName}\beek.exe"; Description: "Launch BeeK"; Flags: nowait postinstall skipifsilent unchecked
[UninstallDelete]
Type: filesandordirs; Name: "{app}\versions"
Type: files; Name: "{app}\install.lock"
[Code]
var InstallLock: THandle; PurgeData: Boolean;
function CreateFileW(Name: String; Access, Share: LongWord; Security: Integer; Creation, Flags: LongWord; Template: Integer): THandle; external 'CreateFileW@kernel32.dll stdcall';
function CloseHandle(Handle: THandle): Boolean; external 'CloseHandle@kernel32.dll stdcall';
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
 Result := '';
 if (InstallLock <> 0) and (InstallLock <> -1) then CloseHandle(InstallLock);
 if not ForceDirectories(ExpandConstant('{app}')) then begin Result := 'Cannot create BeeK directory.'; exit; end;
 InstallLock := CreateFileW(ExpandConstant('{app}\install.lock'), $C0000000, 0, 0, 4, 0, 0);
 if InstallLock = -1 then begin Result := 'BeeK is running or another installer is active. Quit BeeK from its tray menu and retry. No files have been replaced.'; end;
end;
procedure CurStepChanged(CurStep: TSetupStep);
var Old: String;
begin
 if CurStep = ssPostInstall then begin
  // Only after both immutable versioned executables are fully extracted do
  // shortcuts / the existing opted-in Run entry switch to the new pair.
  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK', Old) and (Pos('"' + ExpandConstant('{app}') + '\', Old) = 1) then
    RegWriteStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK', '"' + ExpandConstant('{app}\versions\{#PayloadName}\beek.exe') + '" --autostart');
  if InstallLock <> 0 then begin CloseHandle(InstallLock); InstallLock := 0; end;
 end;
end;
procedure DeinitializeSetup;
begin if (InstallLock <> 0) and (InstallLock <> -1) then CloseHandle(InstallLock); end;
function InitializeUninstall: Boolean;
begin
 PurgeData := False;
 Result := not CheckForMutexes('Local\BeeK-Running');
 if not Result then begin MsgBox('Quit BeeK from its tray menu before uninstalling. No process will be forcibly terminated.', mbError, MB_OK); exit; end;
 if not UninstallSilent then PurgeData := MsgBox('Also permanently delete this Windows user''s BeeK settings, credentials and logs? Choose No to preserve them.', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
end;
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var Code: Integer; Old: String;
begin
 if CurUninstallStep = usUninstall then begin
  InstallLock := CreateFileW(ExpandConstant('{app}\install.lock'), $C0000000, 0, 0, 4, 0, 0);
  if InstallLock = -1 then RaiseException('BeeK started during uninstall. Quit it and retry.');
  if PurgeData then begin
   if not Exec(ExpandConstant('{app}\versions\{#PayloadName}\beek.exe'), '--purge-user-data', '', SW_HIDE, ewWaitUntilTerminated, Code) or (Code <> 0) then RaiseException('BeeK data cleanup failed; files and remaining data preserved.');
  end;
  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK', Old) and (Pos('"' + ExpandConstant('{app}') + '\', Old) = 1) then RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'BeeK');
 end;
 if CurUninstallStep = usPostUninstall then begin if InstallLock <> 0 then begin CloseHandle(InstallLock); InstallLock := 0; end; DeleteFile(ExpandConstant('{app}\install.lock')); end;
end;

```
