# Revision 3: final boundary checks

G5 remains NOT PASSED. Windows testing found that CloseClipboard can advance the sequence after SetClipboardData. The writer now acknowledges the FINAL published sequence after explicit CloseClipboard, with stable owner and sequence checks. Strict tests first seed a DISTINCT clipboard assertion sentinel before copying fields back, preventing old clipboard data from falsely passing an empty-field assertion. Double/triple paste, immediate Cut/Paste, and a real competing process holding the clipboard (field text and original clipboard preserved) all passed on session1. The original weak assertion record has been explicitly marked inconclusive and retained. Current writer tail:
```rust
        EmptyClipboard().map_err(|_| Status::Failed)?;
        SetClipboardData(13, Some(HANDLE(memory.0))).map_err(|_| Status::Failed)?;
        global.1 = true;
        // Windows can advance the sequence again when CloseClipboard publishes
        // the write. Acknowledge the published sequence, with stable ownership,
        // rather than the intermediate value while the clipboard is open.
        drop(_clipboard);
        let after = GetClipboardSequenceNumber();
        if GetClipboardOwner().ok() != Some(owner) || after != GetClipboardSequenceNumber() {
            return Err(Status::SourceChanged);
        }
        Ok(after)
    }
}

```

R2.n1: RestartChild exists ONLY on background creation/supervision threads in lifecycle::restart. Instance::drop never drops that object or joins those threads; it only kills the candidate job. The INFINITE wait intentionally retains the ONE real candidate slot and Busy lease until OS exit is confirmed. Returning after 5 seconds would release Busy while a kernel-stuck candidate still exists, allowing unbounded retry candidates. UI and main shutdown never wait for this background thread; intentional main exit is still bounded by 1900ms. Please verify the complete call chain below and close n1 as a deliberate bounded-resource design, or identify an actual UI/exit blocking call site. R2.n2 is fixed using Jobs::limited(1,None), without a double limit rewrite.

```rust
//! Helpers are attached atomically to kill-on-close jobs at CreateProcess time.
//! There is no spawn -> assign-job orphan window, including main-process crash.
use super::util::{self, Handle};
use crate::core::protocol::{self, Query, Reply};
use std::{
    fs::File,
    io::BufReader,
    os::windows::io::FromRawHandle,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Instant,
};
use windows::{
    core::{BOOL, PCWSTR, PWSTR},
    Win32::{
        Foundation::*,
        Security::SECURITY_ATTRIBUTES,
        Storage::FileSystem::*,
        System::{JobObjects::*, Memory::*, Pipes::*, Threading::*},
    },
};

pub struct Jobs(pub Handle);
impl Jobs {
    pub fn kill(&self) {
        unsafe {
            let _ = TerminateJobObject(self.0 .0, 1);
        }
    }
    pub fn new(limit: u32) -> Result<Arc<Self>, String> {
        Self::limited(limit, Some(256 * 1024 * 1024))
    }
    fn limited(limit: u32, memory: Option<usize>) -> Result<Arc<Self>, String> {
        unsafe {
            let job = Handle(CreateJobObjectW(None, None).map_err(|_| "无法创建助手作业")?);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
            if let Some(memory) = memory {
                info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
                info.ProcessMemoryLimit = memory;
            }
            info.BasicLimitInformation.ActiveProcessLimit = limit;
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
            .map_err(|_| "无法隔离助手作业")?;
            Ok(Arc::new(Self(job)))
        }
    }
}

```

Background restart and Instance Drop call sites:
```rust
    pub fn restart(&mut self) -> Result<(), String> {
        if self.restart_events.is_some() || self.restart_busy.load(Ordering::Acquire) {
            return Err("重启正在进行".into());
        }
        let nonce = util::nonce()?;
        let name = format!(
            "\\\\.\\pipe\\BeeK-restart-{}-{}-{nonce}",
            self.sid, self.identity.session
        );
        let pipe = Pipe::server(&name, &self.sid)?;
        let exe = std::env::current_exe().map_err(|_| "程序路径不可用")?;
        let job = super::process::restart_job()?;
        let cancel = Arc::new(AtomicBool::new(false));
        self.restart_cancel = Some(cancel.clone());
        self.restart_job = Some(job.clone());
        self.restart_committed = false;
        self.restart_busy.store(true, Ordering::Release);
        struct Busy(Arc<AtomicBool>);
        impl Drop for Busy {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let busy = Arc::new(Busy(self.restart_busy.clone()));
        let (tx, rx) = mpsc::sync_channel(2);
        let (grant_tx, grant_rx) = mpsc::sync_channel(1);
        let parent = self.identity;
        thread::spawn(move || {
            let _busy = busy;
            let (created, creation) = mpsc::sync_channel(1);
            let spawn_job = job.clone();
            let spawn_cancel = cancel.clone();
            let spawn_busy = _busy.clone();
            let arguments = format!("--restart-child {} {} {nonce}", parent.pid, parent.created);
            thread::spawn(move || {
                let _busy = spawn_busy;
                let result = super::process::RestartChild::launch(
                    &exe,
                    &arguments,
                    spawn_job,
                    &spawn_cancel,
                );
                let _ = created.send(result);
            });
            let mut child = match creation.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(child)) => child,
                Ok(Err(error)) => {
                    let _ = tx.send(RestartEvent::Failed(error));
                    return;
                }
                Err(_) => {
                    cancel.store(true, Ordering::Release);
                    job.kill();
                    let _ = tx.send(RestartEvent::Failed(
                        "重启创建超时，旧实例继续运行；创建槽实际释放前不能重复重试".into(),
                    ));
                    return;
                }
            };
            let child_pid = child.pid;
            let result = (|| -> Result<(), String> {
                pipe.connect(30_000)?;
                let message: RestartMessage = pipe.read()?;
                let actual = pipe.peer(false)?;
                if message.version != 1
                    || message.nonce != nonce
                    || message.process != actual
                    || actual.pid != child_pid
                    || actual.session != parent.session
                    || message.state != RestartState::ChildReady
                {
                    return Err("重启子进程未就绪，旧进程继续运行".into());
                }
                tx.send(RestartEvent::Ready).map_err(|_| "重启已取消")?;
                grant_rx
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| "重启提交超时")?;
                pipe.write(&RestartMessage {
                    version: 1,
                    nonce: nonce.clone(),
                    process: parent,
                    state: RestartState::ParentExitCommitted,
                })?;
                let accepted: RestartMessage = pipe.read()?;
                if accepted.version != 1
                    || accepted.nonce != nonce
                    || accepted.process != actual
                    || accepted.state != RestartState::ChildCommitAccepted
                {
                    return Err("重启提交未确认，旧进程继续运行".into());
                }
                child.commit()?;
                let _ = tx.send(RestartEvent::Committed);
                Ok(())
            })();
            if let Err(e) = result {
                let _ = tx.send(RestartEvent::Failed(e));
            }
        });
        self.restart_events = Some(rx);
        self.grant = Some(grant_tx);
        Ok(())
    }
    pub fn commit_exit(&mut self) {
        self.restart_committed = true;
    }
    pub fn grant_restart(&mut self) {
        if let Some(tx) = self.grant.take() {
            let _ = tx.try_send(());
        }
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        if !self.restart_committed {
            if let Some(cancel) = &self.restart_cancel {
                cancel.store(true, Ordering::Release);
            }
            if let Some(job) = &self.restart_job {
                job.kill();
            }
        }
        self.stop.store(true, Ordering::Release);
        let at = Instant::now();
        while self.listener.as_ref().is_some_and(|t| !t.is_finished())
            && at.elapsed() < Duration::from_millis(1100)
        {
            thread::sleep(Duration::from_millis(5));
        }
        if self.listener.as_ref().is_some_and(|t| t.is_finished()) {
            let _ = self.listener.take().unwrap().join();
        }
        unsafe {
            let _ = ReleaseMutex(self.mutex.0);
        }
    }
}

```

Source eligibility additionally compares actual TOKEN_MANDATORY_LABEL integrity, not only TokenElevation, while preserving same SID/session and kernel-only reads:
```rust
struct Token {
    sid: Vec<u8>,
    elevated: bool,
    integrity: u32,
}
fn token(process: HANDLE) -> Option<Token> {
    unsafe {
        let mut h = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut h).ok()?;
        let h = Handle(h);
        let mut size = 0;
        let _ = GetTokenInformation(h.0, TokenUser, None, 0, &mut size);
        if size == 0 || size > 64 * 1024 {
            return None;
        }
        let mut bytes = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(
            h.0,
            TokenUser,
            Some(bytes.as_mut_ptr() as *mut _),
            size,
            &mut size,
        )
        .ok()?;
        let sid = (*(bytes.as_ptr() as *const TOKEN_USER)).User.Sid;
        let n = GetLengthSid(sid);
        if n == 0 || n > 256 {
            return None;
        }
        let sid = std::slice::from_raw_parts(sid.0 as *const u8, n as usize).to_vec();
        let mut elevation = TOKEN_ELEVATION::default();
        GetTokenInformation(
            h.0,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of_val(&elevation) as u32,
            &mut size,
        )
        .ok()?;
        let mut needed = 0;
        let _ = GetTokenInformation(h.0, TokenIntegrityLevel, None, 0, &mut needed);
        if needed == 0 || needed > 65536 {
            return None;
        }
        let mut label = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(
            h.0,
            TokenIntegrityLevel,
            Some(label.as_mut_ptr() as *mut _),
            needed,
            &mut needed,
        )
        .ok()?;
        let integrity_sid = (*(label.as_ptr() as *const TOKEN_MANDATORY_LABEL))
            .Label
            .Sid;
        let count = *GetSidSubAuthorityCount(integrity_sid);
        if count == 0 {
            return None;
        }
        let integrity = *GetSidSubAuthority(integrity_sid, (count - 1) as u32);
        Some(Token {
            sid,
            elevated: elevation.TokenIsElevated != 0,
            integrity,
        })
    }
}
fn eligible(process: HANDLE, pid: u32, created: u64) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<(u32, u64), bool>>> = OnceLock::new();
    static OWN: OnceLock<Option<Token>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    if let Some(value) = cache.get(&(pid, created)) {
        return *value;
    }
    let own = OWN.get_or_init(|| token(unsafe { GetCurrentProcess() }));
    let mut session = 0;
    let mut own_session = 0;
    let valid = unsafe {
        ProcessIdToSessionId(pid, &mut session).is_ok()
            && ProcessIdToSessionId(GetCurrentProcessId(), &mut own_session).is_ok()
    } && session == own_session
        && own
            .as_ref()
            .zip(token(process))
            .is_some_and(|(own, other)| {
                own.sid == other.sid
                    && other.integrity <= own.integrity
                    && (!other.elevated || own.elevated)
            });
    if cache.len() >= 64 {
        cache.clear();
    }
    cache.insert((pid, created), valid);
    valid
}

```
