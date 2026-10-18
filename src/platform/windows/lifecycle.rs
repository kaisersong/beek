use super::{
    native::Event,
    util::{self, Handle},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::{BOOL, HRESULT, PCWSTR, PWSTR},
    Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::{Pipes::*, RemoteDesktop::ProcessIdToSessionId, Threading::*, IO::*},
        UI::WindowsAndMessaging::AllowSetForegroundWindow,
    },
};

// Installed payloads live in immutable version directories. This shared read
// handle excludes an installer/uninstaller's exclusive lock for the app lifetime.
pub fn install_guard() -> Result<Option<fs::File>, String> {
    use std::os::windows::fs::OpenOptionsExt;
    let exe = std::env::current_exe().map_err(|_| "程序路径不可用")?;
    let dir = exe.parent().ok_or("程序目录不可用")?;
    if !dir.join("installed.marker").exists() {
        return Ok(None);
    }
    let root = dir
        .parent()
        .filter(|p| p.file_name().is_some_and(|v| v == "versions"))
        .and_then(|p| p.parent())
        .ok_or("安装目录结构无效")?;
    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(root.join("install.lock"))
        .map(Some)
        .map_err(|_| "安装/卸载正在进行，请完成后重试".into())
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    pid: u32,
    created: u64,
    session: u32,
}
fn identity(pid: u32) -> Result<Identity, String> {
    unsafe {
        let p = Handle(
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
            .map_err(|_| "进程身份不可读")?,
        );
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        GetProcessTimes(p.0, &mut c, &mut e, &mut k, &mut u).map_err(|_| "进程创建时间不可读")?;
        let mut session = 0;
        ProcessIdToSessionId(pid, &mut session).map_err(|_| "进程会话不可读")?;
        Ok(Identity {
            pid,
            created: ((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64,
            session,
        })
    }
}
fn me() -> Result<Identity, String> {
    identity(unsafe { GetCurrentProcessId() })
}
struct Security {
    descriptor: PSECURITY_DESCRIPTOR,
}
impl Security {
    fn new(sid: &str) -> Result<Self, String> {
        unsafe {
            let sddl = util::wide(format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})"));
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
            .map_err(|_| "无法设置 IPC 访问控制")?;
            Ok(Self { descriptor })
        }
    }
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.0,
            bInheritHandle: BOOL(0),
        }
    }
}
impl Drop for Security {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
        }
    }
}
fn user_sid() -> Result<String, String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|_| "无法读取用户令牌")?;
        let token = Handle(token);
        let mut needed = 0;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut needed);
        if needed == 0 || needed > 64 * 1024 {
            return Err("用户令牌尺寸无效".into());
        }
        let mut bytes = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(bytes.as_mut_ptr() as *mut _),
            needed,
            &mut needed,
        )
        .map_err(|_| "无法读取用户 SID")?;
        let user = &*(bytes.as_ptr() as *const TOKEN_USER);
        let mut sid = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut sid).map_err(|_| "无法格式化用户 SID")?;
        let result = sid.to_string().map_err(|_| "用户 SID 编码错误".to_owned());
        let _ = LocalFree(Some(HLOCAL(sid.0 as *mut _)));
        result
    }
}
struct Pipe(Handle);
impl Pipe {
    fn server(name: &str, sid: &str) -> Result<Self, String> {
        let security = Security::new(sid)?;
        let name = util::wide(name);
        let attributes = security.attributes();
        let h = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4096,
                4096,
                1000,
                Some(&attributes),
            )
        };
        if h.is_invalid() {
            return Err("无法创建受保护的本地管道".into());
        }
        Ok(Self(Handle(h)))
    }
    fn client(name: &str) -> Result<Self, String> {
        let name = util::wide(name);
        let h = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                0xc0000000,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
        }
        .map_err(|_| "无法连接正在运行的 BeeK")?;
        Ok(Self(Handle(h)))
    }
    fn connect(&self, timeout: u32) -> Result<(), String> {
        unsafe {
            let event =
                Handle(CreateEventW(None, true, false, None).map_err(|_| "IPC 事件不可用")?);
            let mut ov = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            let status = ConnectNamedPipe(self.0 .0, Some(&mut ov));
            if status
                .as_ref()
                .is_err_and(|e| e.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0))
            {
                return Ok(());
            }
            complete(self.0 .0, &mut ov, status, timeout).map(|_| ())
        }
    }
    fn read<T: for<'a> Deserialize<'a>>(&self) -> Result<T, String> {
        self.read_for(1000)
    }
    fn read_for<T: for<'a> Deserialize<'a>>(&self, timeout: u32) -> Result<T, String> {
        unsafe {
            let event =
                Handle(CreateEventW(None, true, false, None).map_err(|_| "IPC 事件不可用")?);
            let mut ov = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            let mut bytes = [0u8; 4097];
            let status = ReadFile(self.0 .0, Some(&mut bytes), None, Some(&mut ov));
            let n = complete(self.0 .0, &mut ov, status, timeout)?;
            if n == 0 || n > 4096 {
                return Err("IPC 消息长度无效".into());
            }
            serde_json::from_slice(&bytes[..n]).map_err(|_| "IPC 消息格式无效".into())
        }
    }
    fn write(&self, value: &impl Serialize) -> Result<(), String> {
        let bytes = serde_json::to_vec(value).map_err(|_| "IPC 消息编码失败")?;
        if bytes.len() > 4096 {
            return Err("IPC 消息过长".into());
        }
        unsafe {
            let event =
                Handle(CreateEventW(None, true, false, None).map_err(|_| "IPC 事件不可用")?);
            let mut ov = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            let status = WriteFile(self.0 .0, Some(&bytes), None, Some(&mut ov));
            if complete(self.0 .0, &mut ov, status, 1000)? != bytes.len() {
                return Err("IPC 消息未完整发送".into());
            }
        }
        Ok(())
    }
    fn peer(&self, server: bool) -> Result<Identity, String> {
        let mut pid = 0;
        unsafe {
            if server {
                GetNamedPipeServerProcessId(self.0 .0, &mut pid)
            } else {
                GetNamedPipeClientProcessId(self.0 .0, &mut pid)
            }
        }
        .map_err(|_| "无法核验管道对端进程")?;
        identity(pid)
    }
}
unsafe fn complete(
    h: HANDLE,
    ov: &mut OVERLAPPED,
    status: windows::core::Result<()>,
    timeout: u32,
) -> Result<usize, String> {
    unsafe {
        if let Err(e) = status {
            if e.code() != windows::core::HRESULT::from_win32(ERROR_IO_PENDING.0) {
                return Err("IPC 操作失败".into());
            }
        }
        if WaitForSingleObject(ov.hEvent, timeout) != WAIT_OBJECT_0 {
            let _ = CancelIoEx(h, Some(ov));
            let mut done = 0;
            let _ = GetOverlappedResult(h, ov, &mut done, true);
            return Err("IPC 操作超时".into());
        }
        let mut n = 0;
        GetOverlappedResult(h, ov, &mut n, false).map_err(|_| "IPC 操作中断")?;
        Ok(n as usize)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    version: u32,
    identity: Identity,
    nonce: String,
    exe: PathBuf,
}
#[derive(Serialize, Deserialize)]
enum PublicCommand {
    Ping,
    ActivateSettings,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicRequest {
    version: u32,
    nonce: String,
    client: Identity,
    command: PublicCommand,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicReply {
    version: u32,
    nonce: String,
    server: Identity,
    ok: bool,
}

pub struct Instance {
    mutex: Handle,
    _running: Handle,
    stop: Arc<AtomicBool>,
    listener: Option<thread::JoinHandle<()>>,
    pub restart_events: Option<mpsc::Receiver<RestartEvent>>,
    grant: Option<mpsc::SyncSender<()>>,
    restart_busy: Arc<AtomicBool>,
    restart_cancel: Option<Arc<AtomicBool>>,
    restart_job: Option<Arc<super::process::Jobs>>,
    restart_committed: bool,
    sid: String,
    identity: Identity,
}
impl Instance {
    pub fn begin(events: mpsc::SyncSender<Event>) -> Result<Option<Self>, String> {
        let sid = user_sid()?;
        let own = me()?;
        let security = Security::new(&sid)?;
        let attributes = security.attributes();
        let name = util::wide(format!("Local\\BeeK-{sid}-{}", own.session));
        let mutex = unsafe { CreateMutexW(Some(&attributes), true, PCWSTR(name.as_ptr())) }
            .map_err(|_| "无法取得 BeeK 会话锁")?;
        let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let mutex = Handle(mutex);
        let pipe_name = format!("\\\\.\\pipe\\BeeK-{sid}-{}", own.session);
        let path = util::known_folder(true)?.join(format!("instance-{}.json", own.session));
        if existing {
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                if let Ok(bytes) = fs::read(&path) {
                    if bytes.len() <= 4096 {
                        if let Ok(owner) = serde_json::from_slice::<Owner>(&bytes) {
                            if owner.version == 1
                                && identity(owner.identity.pid).ok() == Some(owner.identity)
                                && owner.identity.session == own.session
                            {
                                if let Ok(pipe) = Pipe::client(&pipe_name) {
                                    if pipe.peer(true)? != owner.identity {
                                        return Err("BeeK 管道服务身份不匹配".into());
                                    }
                                    unsafe {
                                        let _ = AllowSetForegroundWindow(owner.identity.pid);
                                    }
                                    pipe.write(&PublicRequest {
                                        version: 1,
                                        nonce: owner.nonce.clone(),
                                        client: own,
                                        command: PublicCommand::ActivateSettings,
                                    })?;
                                    let response: PublicReply = pipe.read()?;
                                    if response.version == 1
                                        && response.nonce == owner.nonce
                                        && response.server == owner.identity
                                        && response.ok
                                    {
                                        return Ok(None);
                                    }
                                    return Err("BeeK 会话握手失败".into());
                                }
                            }
                        }
                    }
                }
                if Instant::now() >= deadline {
                    return Err("BeeK 已在启动或暂时无响应，请稍后重试".into());
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
        fs::create_dir_all(path.parent().unwrap()).map_err(|_| "无法创建会话目录")?;
        let owner = Owner {
            version: 1,
            identity: own,
            nonce: util::nonce()?,
            exe: std::env::current_exe().map_err(|_| "程序路径不可用")?,
        };
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, serde_json::to_vec(&owner).unwrap())
            .map_err(|_| "会话记录写入失败")?;
        if path.exists() {
            fs::remove_file(&path).map_err(|_| "旧会话记录无法清理")?;
        }
        fs::rename(temporary, &path).map_err(|_| "会话记录提交失败")?;
        // Create the first pipe before reporting readiness. Each connection uses
        // FIRST_PIPE_INSTANCE and verifies the peer PID, creation time and session.
        let first = Pipe::server(&pipe_name, &sid)?;
        let stop = Arc::new(AtomicBool::new(false));
        let shutdown = stop.clone();
        let thread_sid = sid.clone();
        let listener = thread::spawn(move || {
            let mut next = Some(first);
            let mut create_failures = 0u32;
            while !shutdown.load(Ordering::Acquire) {
                let pipe = match next
                    .take()
                    .or_else(|| Pipe::server(&pipe_name, &thread_sid).ok())
                {
                    Some(p) => {
                        create_failures = 0;
                        p
                    }
                    None => {
                        create_failures = create_failures.saturating_add(1);
                        if create_failures == 50 {
                            super::diagnostics::event(
                                "ipc_unavailable",
                                serde_json::json!({"consecutive_failures":create_failures}),
                            );
                            let _ = events.try_send(Event::IpcUnavailable);
                        }
                        thread::sleep(Duration::from_millis(if create_failures >= 50 {
                            1000
                        } else {
                            20
                        }));
                        continue;
                    }
                };
                if pipe.connect(250).is_err() {
                    continue;
                }
                let request = pipe.read::<PublicRequest>();
                if let Ok(request) = request {
                    let valid = request.version == 1
                        && request.nonce == owner.nonce
                        && request.client.session == own.session
                        && pipe.peer(false).ok() == Some(request.client);
                    if valid {
                        if matches!(request.command, PublicCommand::ActivateSettings) {
                            let _ = events.try_send(Event::Settings);
                        }
                        let _ = pipe.write(&PublicReply {
                            version: 1,
                            nonce: owner.nonce.clone(),
                            server: own,
                            ok: true,
                        });
                    }
                }
            }
        });
        let running = Handle(
            unsafe {
                CreateMutexW(
                    Some(&attributes),
                    true,
                    windows::core::w!("Local\\BeeK-Running"),
                )
            }
            .map_err(|_| "无法创建安装协作锁")?,
        );
        Ok(Some(Self {
            mutex,
            _running: running,
            stop,
            listener: Some(listener),
            restart_events: None,
            grant: None,
            restart_busy: Arc::new(AtomicBool::new(false)),
            restart_cancel: None,
            restart_job: None,
            restart_committed: false,
            sid,
            identity: own,
        }))
    }
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
pub enum RestartEvent {
    Ready,
    Committed,
    Failed(String),
}
#[derive(PartialEq, Eq, Serialize, Deserialize)]
enum RestartState {
    ChildReady,
    ParentExitCommitted,
    ChildCommitAccepted,
    RestartFailed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestartMessage {
    version: u32,
    nonce: String,
    process: Identity,
    state: RestartState,
}

pub fn child_preflight() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--restart-child") {
        return Ok(());
    }
    if args.len() != 5 || args[4].len() != 48 || !args[4].bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("重启参数无效".into());
    }
    let pid = args[2].parse().map_err(|_| "重启父进程无效")?;
    let created: u64 = args[3].parse().map_err(|_| "重启父进程创建时间无效")?;
    let parent = identity(pid)?;
    let own = me()?;
    if parent.created != created || parent.session != own.session {
        return Err("重启父进程身份不匹配".into());
    }
    super::pair::helper()?;
    super::store::Store::open()?.load()?;
    let sid = user_sid()?;
    let pipe = Pipe::client(&format!(
        "\\\\.\\pipe\\BeeK-restart-{sid}-{}-{}",
        own.session, args[4]
    ))?;
    if pipe.peer(true)? != parent {
        return Err("重启管道父进程身份无效".into());
    }
    let parent_handle = Handle(
        unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
        }
        .map_err(|_| "重启父进程不可读")?,
    );
    pipe.write(&RestartMessage {
        version: 1,
        nonce: args[4].clone(),
        process: own,
        state: RestartState::ChildReady,
    })?;
    let response: RestartMessage = pipe.read_for(6000)?;
    if response.version != 1
        || response.nonce != args[4]
        || response.process != parent
        || response.state != RestartState::ParentExitCommitted
    {
        return Err("重启未得到父进程提交".into());
    }
    pipe.write(&RestartMessage {
        version: 1,
        nonce: args[4].clone(),
        process: own,
        state: RestartState::ChildCommitAccepted,
    })?;
    if unsafe { WaitForSingleObject(parent_handle.0, 10_000) } != WAIT_OBJECT_0 {
        return Err("旧进程未按时退出，子进程已取消启动".into());
    }
    Ok(())
}
pub fn exit_deadline() {
    // Only bounds an intentional shutdown; never restarts a crash.
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(1900));
        std::process::exit(0);
    });
}
