# Read-only review snapshot

Native event callbacks never query UIA. The parent limits a total query including CreateProcess to 1500ms; starting and terminating slots stay counted until actual termination. UIA max2, clipboard1, shared6 attempts/10s. G5 is not accepted and signing/multiple-display/clean-machine/8h evidence are pending. This pass reviews process/UIA/protocol only; UI/storage/network/install are reviewed separately.

## src/core/protocol.rs
```rust
//! Bounded, versioned messages. Selection content belongs only in private IPC,
//! never diagnostic logs, command-line arguments or persistent evidence.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, BufRead, Read, Write};

pub const VERSION: u32 = 1;
pub const MAX_FRAME: usize = 256 * 1024;
pub const MAX_CHARS: usize = 16_384;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub hwnd: u64,
    pub pid: u32,
    pub created: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    pub fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|n| n.is_finite() && n.abs() < 10_000_000.)
            && self.width > 0.
            && self.height > 0.
    }
    pub fn clamp_popup(self, width: f64, height: f64, work: Self) -> Self {
        let width = width.max(1.).min(work.width);
        let height = height.max(1.).min(work.height);
        Self {
            x: self.x.max(work.x).min(work.x + work.width - width),
            y: (self.y + self.height + 6.)
                .max(work.y)
                .min(work.y + work.height - height),
            width,
            height,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub source: Source,
    pub epoch: u64,
    pub runtime_id: Vec<i32>,
    pub text: String,
    pub rects: Vec<Rect>,
    pub clipboard_sequence: Option<u32>,
}
impl Selection {
    pub fn validate(&self) -> bool {
        self.source.pid != 0
            && self.source.created != 0
            && self.runtime_id.len() <= 128
            && !self.text.trim().is_empty()
            && self.text.chars().count() <= MAX_CHARS
            && self.rects.len() <= 1024
            && self.rects.iter().all(|r| r.valid())
            && (self.clipboard_sequence.is_some() || !self.rects.is_empty())
    }
    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.text.as_bytes()).into()
    }
    pub fn same_selection(&self, fresh: &Self) -> bool {
        self.source == fresh.source
            && self.epoch == fresh.epoch
            && self.runtime_id == fresh.runtime_id
            && self.clipboard_sequence == fresh.clipboard_sequence
            && self.digest() == fresh.digest()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedBuffer {
    pub handle: u64,
    pub bytes: u32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub version: u32,
    pub nonce: String,
    pub id: u64,
    pub source: Source,
    pub epoch: u64,
    pub point: [i32; 2],
    pub pointer_anchor: bool,
    pub clipboard: bool,
    pub explicit_clipboard: bool,
    pub write_buffer: Option<SharedBuffer>,
    pub clipboard_sequence: Option<u32>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub version: u32,
    pub build: String,
    pub nonce: String,
    pub id: u64,
    pub source: Source,
    pub selection: Option<Selection>,
    pub status: Status,
    pub clipboard_sequence: Option<u32>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum Status {
    Ready,
    Selected,
    Empty,
    Unsupported,
    Protected,
    SourceChanged,
    Busy,
    Failed,
    Alive,
    Changed,
    ClipboardWritten,
}

pub fn read_frame<T: for<'a> Deserialize<'a>>(reader: &mut impl BufRead) -> io::Result<T> {
    let mut line = Vec::with_capacity(4096);
    // Limit WHILE reading, not after an unbounded allocation.
    reader
        .take((MAX_FRAME + 1) as u64)
        .read_until(b'\n', &mut line)?;
    if line.len() > MAX_FRAME || line.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid IPC frame size or termination",
        ));
    }
    serde_json::from_slice(&line)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid IPC schema"))
}
pub fn write_frame(value: &impl Serialize, writer: &mut impl Write) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| io::Error::other("IPC serialization"))?;
    if bytes.len() >= MAX_FRAME {
        return Err(io::Error::other("IPC frame too large"));
    }
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_and_unterminated_input_is_rejected_before_parsing() {
        let mut huge = io::Cursor::new(vec![b'x'; MAX_FRAME * 3]);
        assert!(read_frame::<serde_json::Value>(&mut huge).is_err());
        assert_eq!(huge.position(), (MAX_FRAME + 1) as u64);
        assert!(read_frame::<serde_json::Value>(&mut io::Cursor::new(b"{}")).is_err());
    }
    #[test]
    fn geometry_preserves_negative_monitor_coordinates() {
        let work = Rect {
            x: -1920.,
            y: -300.,
            width: 1920.,
            height: 1080.,
        };
        let p = Rect {
            x: -4.,
            y: 770.,
            width: 4.,
            height: 12.,
        }
        .clamp_popup(480., 220., work);
        assert_eq!(p.x, -480.);
        assert_eq!(p.y, 560.);
        assert!(p.valid());
    }
}

```

## src/core/policy.rs
```rust
use super::protocol::Source;
use std::collections::{HashMap, VecDeque};

pub const QUERY_MS: u64 = 1500;
pub const PENDING_MS: u64 = 300;
pub const TTL_MS: u64 = 8000;

#[derive(Default)]
struct Fault {
    failures: u8,
    next: u64,
    quarantined: bool,
    probe_used: bool,
}
#[derive(Default)]
pub struct Health {
    sources: HashMap<(u32, u64), Fault>,
    attempts: VecDeque<u64>,
    infrastructure: u8,
    cross_source: Vec<(u32, u64)>,
    paused: bool,
}
impl Health {
    pub fn allowed(&self, source: Source, now: u64) -> bool {
        !self.paused
            && self
                .sources
                .get(&(source.pid, source.created))
                .is_none_or(|f| {
                    !f.quarantined && now >= f.next && !(f.failures >= 3 && f.probe_used)
                })
    }
    pub fn begin(&mut self, source: Source, now: u64) -> bool {
        if !self.allowed(source, now) {
            return false;
        }
        if let Some(f) = self.sources.get_mut(&(source.pid, source.created)) {
            if f.failures >= 3 {
                f.probe_used = true;
            }
        }
        true
    }
    pub fn attempt(&mut self, now: u64) -> bool {
        while self
            .attempts
            .front()
            .is_some_and(|t| now.saturating_sub(*t) >= 10_000)
        {
            self.attempts.pop_front();
        }
        if self.attempts.len() >= 6 {
            return false;
        }
        self.attempts.push_back(now);
        true
    }
    pub fn success(&mut self, source: Source) {
        self.sources.remove(&(source.pid, source.created));
        self.infrastructure = 0;
        self.cross_source.clear();
    }
    pub fn failure(&mut self, source: Option<Source>, now: u64, protocol_fault: bool) {
        if let Some(s) = source {
            let key = (s.pid, s.created);
            let f = self.sources.entry(key).or_default();
            f.failures = f.failures.saturating_add(1);
            match f.failures {
                1 => f.next = now + 1000,
                2 => f.next = now + 5000,
                3 => f.next = now + 30_000,
                _ => f.quarantined = true,
            }
            if protocol_fault && !self.cross_source.contains(&key) {
                self.cross_source.push(key);
                if self.cross_source.len() >= 3 {
                    self.paused = true;
                }
            }
        } else {
            self.infrastructure = self.infrastructure.saturating_add(1);
            if self.infrastructure >= 3 {
                self.paused = true;
            }
        }
    }
    pub fn restore(&mut self) {
        self.sources.clear();
        self.infrastructure = 0;
        self.cross_source.clear();
        self.paused = false;
    }
    pub fn retain_live(&mut self, mut alive: impl FnMut(u32, u64) -> bool) {
        self.sources
            .retain(|(pid, created), _| alive(*pid, *created));
        self.cross_source
            .retain(|(pid, created)| alive(*pid, *created));
    }
    pub fn summary(&self, now: u64) -> (usize, usize, usize) {
        (
            self.sources.len(),
            self.sources.values().filter(|f| f.quarantined).count(),
            self.attempts
                .iter()
                .filter(|t| now.saturating_sub(**t) < 10000)
                .count(),
        )
    }
    pub fn paused(&self) -> bool {
        self.paused
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn src(pid: u32) -> Source {
        Source {
            hwnd: pid as u64,
            pid,
            created: 10,
            generation: 1,
        }
    }
    #[test]
    fn one_hung_application_never_disables_other_applications() {
        let mut h = Health::default();
        let s = src(1);
        for (now, wait) in [(0, 1000), (1000, 5000), (6000, 30_000)] {
            h.failure(Some(s), now, false);
            assert!(!h.allowed(s, now + wait - 1));
            assert!(h.allowed(s, now + wait));
            assert!(h.allowed(src(2), now));
        }
        h.failure(Some(s), 36_000, false);
        assert!(!h.allowed(s, 1_000_000));
        assert!(h.allowed(Source { created: 11, ..s }, 36_000));
        assert!(!h.paused());
    }
    #[test]
    fn spawn_budget_is_shared_and_counts_failed_attempts() {
        let mut h = Health::default();
        for _ in 0..6 {
            assert!(h.attempt(0));
        }
        assert!(!h.attempt(9999));
        h.restore();
        assert!(!h.attempt(9999));
        assert!(h.attempt(10_000));
    }
    #[test]
    fn three_distinct_protocol_faults_pause_platform_but_not_repeated_one_source() {
        let mut h = Health::default();
        for _ in 0..4 {
            h.failure(Some(src(1)), 0, true);
        }
        assert!(!h.paused());
        h.failure(Some(src(2)), 0, true);
        h.failure(Some(src(3)), 0, true);
        assert!(h.paused());
        h.restore();
        assert!(h.allowed(src(1), 0));
    }
    #[test]
    fn exactly_one_probe_and_restore_cannot_bypass_spawn_budget() {
        let mut h = Health::default();
        let s = src(7);
        for now in [0, 1000, 6000] {
            h.failure(Some(s), now, false);
        }
        assert!(!h.begin(s, 35999));
        assert!(h.begin(s, 36000));
        assert!(!h.begin(s, 36001));
        h.failure(Some(s), 36002, false);
        assert!(!h.begin(s, u64::MAX));
        for _ in 0..6 {
            assert!(h.attempt(36002));
        }
        h.restore();
        assert!(h.allowed(s, 36002));
        assert!(!h.attempt(36003));
    }
    #[test]
    fn infrastructure_pause_still_allows_bounded_explicit_clipboard() {
        let mut h = Health::default();
        for _ in 0..3 {
            h.failure(None, 0, true);
        }
        assert!(h.paused());
        assert!(!h.begin(src(1), 0));
        assert!(h.attempt(0));
        h.restore();
        assert!(h.begin(src(1), 0));
    }
}

```

## src/platform/windows/process.rs
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
        unsafe {
            let job = Handle(CreateJobObjectW(None, None).map_err(|_| "无法创建助手作业")?);
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            info.ProcessMemoryLimit = 256 * 1024 * 1024;
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
pub struct Child {
    pub query: Query,
    pub start: Instant,
    pub process: Handle,
    pub job: Arc<Jobs>,
    pub reader: Option<thread::JoinHandle<()>>,
    pub result: mpsc::Receiver<Result<Reply, String>>,
    pub input: File,
    _main_job: Arc<Jobs>,
}
impl Child {
    pub fn spawn(
        mut query: Query,
        copy: Option<String>,
        main_job: Arc<Jobs>,
        job: Arc<Jobs>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        unsafe {
            let start = Instant::now();
            let helper = super::pair::helper()?;
            let attrs = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: std::ptr::null_mut(),
                bInheritHandle: BOOL(1),
            };
            let (mut ir, mut iw, mut or, mut ow) = (
                HANDLE::default(),
                HANDLE::default(),
                HANDLE::default(),
                HANDLE::default(),
            );
            CreatePipe(&mut ir, &mut iw, Some(&attrs), 0).map_err(|_| "无法创建助手输入管道")?;
            let stdin_child = Handle(ir);
            let stdin_parent = Handle(iw);
            CreatePipe(&mut or, &mut ow, Some(&attrs), 0).map_err(|_| "无法创建助手输出管道")?;
            let stdout_parent = Handle(or);
            let stdout_child = Handle(ow);
            SetHandleInformation(stdin_parent.0, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))
                .map_err(|_| "管道隔离失败")?;
            SetHandleInformation(stdout_parent.0, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))
                .map_err(|_| "管道隔离失败")?;
            let mut length = 0;
            let _ = InitializeProcThreadAttributeList(None, 2, None, &mut length);
            // usize allocation supplies pointer alignment required by the OS attribute list.
            let mut storage = vec![0usize; length.div_ceil(std::mem::size_of::<usize>())];
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr() as *mut _);
            InitializeProcThreadAttributeList(Some(list), 2, None, &mut length)
                .map_err(|_| "助手属性初始化失败")?;
            struct Attributes(LPPROC_THREAD_ATTRIBUTE_LIST);
            impl Drop for Attributes {
                fn drop(&mut self) {
                    unsafe {
                        DeleteProcThreadAttributeList(self.0);
                    }
                }
            }
            let _attributes = Attributes(list);
            let handles = [stdin_child.0, stdout_child.0];
            let jobs = [main_job.0 .0, job.0 .0];
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                Some(handles.as_ptr() as *const _),
                std::mem::size_of_val(&handles),
                None,
                None,
            )
            .map_err(|_| "助手句柄白名单失败")?;
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                Some(jobs.as_ptr() as *const _),
                std::mem::size_of_val(&jobs),
                None,
                None,
            )
            .map_err(|_| "助手原子作业隔离失败")?;
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
            startup.StartupInfo.hStdInput = stdin_child.0;
            startup.StartupInfo.hStdOutput = stdout_child.0;
            startup.StartupInfo.hStdError = stdout_child.0;
            startup.lpAttributeList = list;
            let app = util::wide(&helper);
            let command = format!("\"{}\" --nonce {}", helper.display(), query.nonce);
            let mut command = util::wide(command);
            let mut info = PROCESS_INFORMATION::default();
            if cancel.load(Ordering::Acquire) {
                return Err("助手启动已取消".into());
            }
            CreateProcessW(
                PCWSTR(app.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                None,
                None,
                true,
                CREATE_NO_WINDOW | CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
                None,
                None,
                &startup.StartupInfo,
                &mut info,
            )
            .map_err(|_| "助手进程启动失败")?;
            let process = Handle(info.hProcess);
            let thread = Handle(info.hThread);
            if cancel.load(Ordering::Acquire) {
                let _ = TerminateJobObject(job.0 .0, 1);
                return Err("助手启动已取消".into());
            }
            attach_copy(&mut query, copy.as_deref(), process.0)?;
            super::diagnostics::event(
                "worker_spawned",
                serde_json::json!({"pid":info.dwProcessId,"source_pid":query.source.pid,"clipboard":query.clipboard,"atomic_job":true}),
            );
            if ResumeThread(thread.0) == u32::MAX {
                let _ = TerminateJobObject(job.0 .0, 1);
                return Err("助手线程启动失败".into());
            }
            drop(thread);
            drop(stdin_child);
            drop(stdout_child);
            let raw = stdin_parent.0;
            std::mem::forget(stdin_parent);
            let mut writer = File::from_raw_handle(raw.0);
            if protocol::write_frame(&query, &mut writer).is_err() {
                let _ = TerminateJobObject(job.0 .0, 1);
                return Err("助手查询发送失败".into());
            }
            let raw = stdout_parent.0;
            std::mem::forget(stdout_parent);
            let output = File::from_raw_handle(raw.0);
            let (tx, result) = mpsc::sync_channel(16);
            let reader = thread::spawn(move || {
                let mut reader = BufReader::new(output);
                loop {
                    let reply =
                        protocol::read_frame(&mut reader).map_err(|_| "助手响应协议错误".into());
                    let failed = reply.is_err();
                    if tx.send(reply).is_err() || failed {
                        break;
                    }
                }
            });
            Ok(Self {
                query,
                start,
                process,
                job,
                reader: Some(reader),
                result,
                input: writer,
                _main_job: main_job,
            })
        }
    }
    pub fn kill(&self) {
        unsafe {
            let _ = TerminateJobObject(self.job.0 .0, 1);
        }
    }
    pub fn send(&mut self, mut query: Query, copy: Option<String>) -> Result<(), String> {
        attach_copy(&mut query, copy.as_deref(), self.process.0)?;
        protocol::write_frame(&query, &mut self.input).map_err(|_| "助手查询发送失败")?;
        self.query = query;
        self.start = Instant::now();
        Ok(())
    }
    pub fn exited(&self) -> bool {
        unsafe { WaitForSingleObject(self.process.0, 0) == WAIT_OBJECT_0 }
    }
    pub fn reaped(&mut self) -> bool {
        if !self.exited() || self.reader.as_ref().is_some_and(|r| !r.is_finished()) {
            return false;
        }
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        true
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        self.kill();
    }
}
fn attach_copy(query: &mut Query, text: Option<&str>, process: HANDLE) -> Result<(), String> {
    let Some(text) = text else {
        return Ok(());
    };
    if text.is_empty() || text.len() > 256 * 1024 || text.contains('\0') {
        return Err("译文为空、过长或含无效字符".into());
    }
    unsafe {
        let mapping = Handle(
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                text.len() as u32,
                None,
            )
            .map_err(|_| "无法创建复制缓冲区")?,
        );
        let view = MapViewOfFile(mapping.0, FILE_MAP_WRITE, 0, 0, text.len());
        if view.Value.is_null() {
            return Err("无法映射复制缓冲区".into());
        }
        std::ptr::copy_nonoverlapping(text.as_ptr(), view.Value as *mut u8, text.len());
        let _ = UnmapViewOfFile(view);
        let mut child = HANDLE::default();
        DuplicateHandle(
            GetCurrentProcess(),
            mapping.0,
            process,
            &mut child,
            FILE_MAP_READ.0,
            false,
            DUPLICATE_HANDLE_OPTIONS(0),
        )
        .map_err(|_| "无法授权复制缓冲区")?;
        query.write_buffer = Some(protocol::SharedBuffer {
            handle: child.0 as u64,
            bytes: text.len() as u32,
        });
    }
    Ok(())
}

```

## src/platform/windows/selection.rs
```rust
use super::{
    process::{Child, Jobs},
    util,
};
use crate::core::{
    policy::{Health, PENDING_MS, QUERY_MS},
    protocol::{Query, Selection, Source, Status, VERSION},
};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub enum Outcome {
    Selected(u64, Selection),
    Empty(u64, Status),
    Fault(u64, &'static str),
    Paused,
    Copied(u64, u32),
}
struct Pending {
    query: Query,
    at: Instant,
    copy: Option<String>,
}
struct Worker {
    child: Child,
    busy: Option<Instant>,
    last_seen: Instant,
    terminating: bool,
    handshaken: bool,
}
struct Starting {
    query: Query,
    at: Instant,
    job: Arc<Jobs>,
    cancel: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    result: mpsc::Receiver<Result<Child, String>>,
    terminating: bool,
}
enum Slot {
    Starting(Starting),
    Running(Worker),
    Gone,
}
impl Slot {
    fn query(&self) -> Option<&Query> {
        match self {
            Self::Starting(s) => Some(&s.query),
            Self::Running(w) => Some(&w.child.query),
            Self::Gone => None,
        }
    }
    fn terminating(&self) -> bool {
        match self {
            Self::Starting(s) => s.terminating,
            Self::Running(w) => w.terminating,
            Self::Gone => true,
        }
    }
    fn oldest(&self) -> Instant {
        match self {
            Self::Starting(s) => s.at,
            Self::Running(w) => w.child.start,
            Self::Gone => Instant::now(),
        }
    }
    fn kill(&mut self) {
        match self {
            Self::Starting(s) => {
                s.cancel.store(true, Ordering::Release);
                s.job.kill();
                s.terminating = true;
            }
            Self::Running(w) => {
                w.child.kill();
                w.terminating = true;
            }
            Self::Gone => {}
        }
    }
}
fn emit(result: &Arc<Mutex<VecDeque<Outcome>>>, outcome: Outcome) {
    let mut queue = result.lock().unwrap();
    if queue.len() >= 16 {
        queue.pop_front();
        super::diagnostics::event("outcome_overflow", serde_json::json!({"dropped":1}));
    }
    queue.push_back(outcome);
}

fn failure(health: &mut Health, query: &Query, handshaken: bool, now: u64, protocol_fault: bool) {
    super::diagnostics::event(
        "worker_fault",
        serde_json::json!({"id":query.id,"source_pid":query.source.pid,"clipboard":query.clipboard,"handshaken":handshaken,"protocol_fault":protocol_fault}),
    );
    // A clipboard renderer is not attributed to the foreground UIA provider.
    // All process creation attempts still consume the one shared rolling budget.
    if query.clipboard && handshaken {
        return;
    }
    health.failure(
        if handshaken { Some(query.source) } else { None },
        now,
        protocol_fault,
    );
}
fn stop_worker(
    w: &mut Worker,
    health: &mut Health,
    result: &Arc<Mutex<VecDeque<Outcome>>>,
    now: u64,
    code: &'static str,
    protocol_fault: bool,
) {
    if !w.terminating {
        failure(health, &w.child.query, w.handshaken, now, protocol_fault);
        emit(result, Outcome::Fault(w.child.query.id, code));
        w.child.kill();
        w.terminating = true;
    }
}
fn poll_slot(
    slot: &mut Slot,
    health: &mut Health,
    result: &Arc<Mutex<VecDeque<Outcome>>>,
    now: u64,
) {
    if let Slot::Starting(s) = slot {
        if !s.terminating && s.at.elapsed() >= Duration::from_millis(QUERY_MS) {
            failure(health, &s.query, false, now, false);
            emit(result, Outcome::Fault(s.query.id, "worker_start_timeout"));
            s.cancel.store(true, Ordering::Release);
            s.job.kill();
            s.terminating = true;
        }
        if s.thread.as_ref().is_some_and(|t| t.is_finished()) {
            let _ = s.thread.take().unwrap().join();
            let reply = s.result.try_recv();
            match reply {
                Ok(Ok(child)) => {
                    if s.terminating {
                        child.kill();
                    }
                    *slot = Slot::Running(Worker {
                        child,
                        busy: Some(s.at),
                        last_seen: Instant::now(),
                        terminating: s.terminating,
                        handshaken: false,
                    });
                }
                _ => {
                    if !s.terminating {
                        failure(health, &s.query, false, now, true);
                        emit(result, Outcome::Fault(s.query.id, "worker_start"));
                    }
                    *slot = Slot::Gone;
                }
            }
        }
    }
    if let Slot::Running(w) = slot {
        if !w.terminating
            && (w
                .busy
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(QUERY_MS))
                || w.last_seen.elapsed() >= Duration::from_millis(QUERY_MS))
        {
            stop_worker(w, health, result, now, "worker_timeout", false);
        }
        // Drain while terminating too, so a bounded reader queue cannot keep the
        // reader thread alive and prevent confirmed reap of an already dead process.
        while let Ok(reply) = w.child.result.try_recv() {
            let reply = match reply {
                Ok(r) => r,
                Err(_) => {
                    stop_worker(w, health, result, now, "worker_protocol_or_exit", true);
                    break;
                }
            };
            if w.terminating {
                continue;
            }
            if reply.id < w.child.query.id && reply.nonce == w.child.query.nonce {
                continue;
            }
            let selected = reply.status == Status::Selected;
            if reply.version != VERSION
                || reply.build != env!("BEEK_BUILD_ID")
                || reply.nonce != w.child.query.nonce
                || reply.id != w.child.query.id
                || reply.source != w.child.query.source
                || (reply.status == Status::ClipboardWritten
                    && w.child.query.write_buffer.is_none())
                || selected != reply.selection.is_some()
                || (reply.status == Status::ClipboardWritten) != reply.clipboard_sequence.is_some()
                || reply.selection.as_ref().is_some_and(|s| {
                    !s.validate() || s.source != reply.source || s.epoch != w.child.query.epoch
                })
            {
                stop_worker(w, health, result, now, "worker_protocol", true);
                break;
            }
            if w.busy
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(QUERY_MS))
            {
                stop_worker(w, health, result, now, "worker_timeout", false);
                continue;
            }
            w.last_seen = Instant::now();
            match reply.status {
                Status::Ready => w.handshaken = true,
                Status::Alive => {}
                Status::Changed => {
                    if util::is_current(reply.source) {
                        super::native::selection_changed();
                    }
                }
                status => {
                    if !w.handshaken || w.busy.is_none() {
                        stop_worker(w, health, result, now, "worker_protocol", true);
                        continue;
                    }
                    w.busy = None;
                    if status == Status::Failed {
                        w.child.kill();
                        w.terminating = true;
                    }
                    if !w.child.query.clipboard {
                        if matches!(
                            status,
                            Status::Selected
                                | Status::Empty
                                | Status::Unsupported
                                | Status::Protected
                        ) {
                            health.success(reply.source);
                        }
                    }
                    super::diagnostics::event(
                        "selection_reply",
                        serde_json::json!({"id":reply.id,"source_pid":reply.source.pid,"status":format!("{status:?}"),"clipboard":w.child.query.clipboard}),
                    );
                    emit(
                        result,
                        if status == Status::ClipboardWritten {
                            Outcome::Copied(reply.id, reply.clipboard_sequence.unwrap())
                        } else if let Some(s) = reply.selection {
                            Outcome::Selected(reply.id, s)
                        } else {
                            Outcome::Empty(reply.id, status)
                        },
                    );
                }
            }
        }
        if !w.terminating && w.child.exited() {
            stop_worker(w, health, result, now, "worker_crash", true);
        }
        if !w.terminating && !util::identity_alive(w.child.query.source) {
            w.child.kill();
            w.terminating = true;
        }
        if w.terminating && w.child.reaped() {
            *slot = Slot::Gone;
        }
    }
}
fn launch(pending: Pending, main: Arc<Jobs>) -> Result<Slot, String> {
    let job = Jobs::new(1)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, result) = mpsc::sync_channel(1);
    let (query, at) = (pending.query.clone(), pending.at);
    let child_job = job.clone();
    let cancelled = cancel.clone();
    let thread = thread::spawn(move || {
        #[cfg(feature = "test-fixtures")]
        if std::env::var("BEEK_TEST_SPAWN_DELAY_MS").ok().as_deref() == Some("2200") {
            thread::sleep(Duration::from_millis(2200));
        }
        let child = Child::spawn(pending.query, pending.copy, main, child_job, cancelled);
        let _ = tx.send(child);
    });
    Ok(Slot::Starting(Starting {
        query,
        at,
        job,
        cancel,
        thread: Some(thread),
        result,
        terminating: false,
    }))
}
pub struct Coordinator {
    stats: Arc<Mutex<serde_json::Value>>,
    pending: Arc<Mutex<Option<Pending>>>,
    pub result: Arc<Mutex<VecDeque<Outcome>>>,
    stop: Arc<AtomicBool>,
    restore: Arc<AtomicBool>,
    nonce: String,
}
impl Coordinator {
    pub fn start() -> Result<Self, String> {
        let jobs = Jobs::new(3)?;
        let nonce = util::nonce()?;
        let pending = Arc::new(Mutex::new(None::<Pending>));
        let result = Arc::new(Mutex::new(VecDeque::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let restore = Arc::new(AtomicBool::new(false));
        let (p, r, s, reset) = (
            pending.clone(),
            result.clone(),
            stop.clone(),
            restore.clone(),
        );
        let stats = Arc::new(Mutex::new(serde_json::json!({})));
        let thread_stats = stats.clone();
        thread::spawn(move || {
            let clock = Instant::now();
            let mut health = Health::default();
            let mut slots = Vec::<Slot>::new();
            let mut waiting: Option<Pending> = None;
            let mut last_prune = Instant::now();
            let mut reported_pause = false;
            let mut system_epoch = super::native::SYSTEM_EPOCH.load(Ordering::Acquire);
            while !s.load(Ordering::Acquire) {
                let context = super::native::SYSTEM_EPOCH.load(Ordering::Acquire);
                if context != system_epoch {
                    system_epoch = context;
                    waiting = None;
                    let _ = p.lock().unwrap().take();
                    for slot in &mut slots {
                        slot.kill();
                    }
                }
                if reset.swap(false, Ordering::AcqRel) {
                    health.restore();
                }
                let newest = { p.lock().unwrap().take() };
                if let Some(new) = newest {
                    if let Some(old) = waiting.replace(new) {
                        emit(&r, Outcome::Fault(old.query.id, "superseded"));
                    }
                }
                let now = clock.elapsed().as_millis() as u64;
                for slot in &mut slots {
                    poll_slot(slot, &mut health, &r, now);
                }
                slots.retain(|s| !matches!(s, Slot::Gone));
                if last_prune.elapsed() >= Duration::from_secs(10) {
                    health.retain_live(|pid, created| util::process_alive(pid, created));
                    last_prune = Instant::now();
                }
                if health.paused() {
                    if !reported_pause {
                        emit(&r, Outcome::Paused);
                        reported_pause = true;
                    }
                    if waiting.as_ref().is_some_and(|p| !p.query.clipboard) {
                        emit(&r, Outcome::Paused);
                        waiting = None;
                    }
                    for slot in &mut slots {
                        if slot.query().is_some_and(|q| !q.clipboard) {
                            slot.kill();
                        }
                    }
                } else {
                    reported_pause = false;
                }
                if let Some(q) = waiting.as_ref() {
                    if q.at.elapsed() >= Duration::from_millis(PENDING_MS) {
                        emit(&r, Outcome::Fault(q.query.id, "pending_expired"));
                        waiting = None;
                    } else if !(if q.query.explicit_clipboard {
                        util::identity_alive(q.query.source)
                    } else {
                        util::is_current(q.query.source)
                    }) || (!q.query.clipboard && !health.allowed(q.query.source, now))
                    {
                        emit(&r, Outcome::Empty(q.query.id, Status::SourceChanged));
                        waiting = None;
                    } else {
                        let clipboard = q.query.clipboard;
                        let reusable = slots.iter().position(|slot| match slot {
                            Slot::Running(w) => {
                                !w.terminating
                                    && w.busy.is_none()
                                    && w.child.query.clipboard == clipboard
                                    && (clipboard
                                        || (w.child.query.source.pid == q.query.source.pid
                                            && w.child.query.source.created
                                                == q.query.source.created))
                            }
                            _ => false,
                        });
                        if let Some(index) = reusable {
                            if !clipboard && !health.begin(q.query.source, now) {
                                waiting = None;
                                continue;
                            }
                            let q = waiting.take().unwrap();
                            if let Slot::Running(w) = &mut slots[index] {
                                if w.child.send(q.query, q.copy).is_ok() {
                                    w.busy = Some(q.at);
                                    w.last_seen = Instant::now();
                                } else {
                                    stop_worker(w, &mut health, &r, now, "worker_send", true);
                                }
                            }
                        } else if let Some(index) = slots.iter().position(|s| {
                            s.query().is_some_and(|old| {
                                old.clipboard == clipboard
                                    && old.source.pid == q.query.source.pid
                                    && old.source.created == q.query.source.created
                            })
                        }) {
                            let stale = slots[index].query().is_some_and(|old| {
                                if clipboard {
                                    old.clipboard_sequence != q.query.clipboard_sequence
                                } else {
                                    old.source != q.query.source || old.epoch != q.query.epoch
                                }
                            });
                            if stale && !slots[index].terminating() {
                                slots[index].kill();
                            }
                        } else {
                            let count = slots
                                .iter()
                                .filter(|s| s.query().is_some_and(|q| q.clipboard == clipboard))
                                .count();
                            if count < if clipboard { 1 } else { 2 } {
                                if health.attempt(now) {
                                    if !clipboard && !health.begin(q.query.source, now) {
                                        waiting = None;
                                        continue;
                                    }
                                    let q = waiting.take().unwrap();
                                    let id = q.query.id;
                                    match launch(q, jobs.clone()) {
                                        Ok(slot) => slots.push(slot),
                                        Err(_) => {
                                            health.failure(None, now, true);
                                            emit(&r, Outcome::Fault(id, "worker_start"));
                                        }
                                    }
                                }
                            } else {
                                if let Some(slot) = slots
                                    .iter_mut()
                                    .filter(|s| {
                                        s.query().is_some_and(|old| {
                                            old.clipboard == clipboard
                                                && (if clipboard {
                                                    old.clipboard_sequence
                                                        != q.query.clipboard_sequence
                                                } else {
                                                    old.source != q.query.source
                                                        || old.epoch != q.query.epoch
                                                        || !util::is_current(old.source)
                                                })
                                        }) && !s.terminating()
                                    })
                                    .min_by_key(|s| s.oldest())
                                {
                                    slot.kill();
                                }
                            }
                        }
                    }
                }
                let (sources, quarantined, attempts) = health.summary(now);
                *thread_stats.lock().unwrap() = serde_json::json!({"uia_slots":slots.iter().filter(|s|s.query().is_some_and(|q|!q.clipboard)).count(),"clipboard_slots":slots.iter().filter(|s|s.query().is_some_and(|q|q.clipboard)).count(),"terminating":slots.iter().filter(|s|s.query().is_some()&&s.terminating()).count(),"attempts_last_10s":attempts,"fault_sources":sources,"quarantined_sources":quarantined,"paused":health.paused()});
                thread::sleep(Duration::from_millis(10));
            }
            for slot in &mut slots {
                slot.kill();
            }
        });
        Ok(Self {
            stats,
            pending,
            result,
            stop,
            restore,
            nonce,
        })
    }
    pub fn snapshot(&self) -> serde_json::Value {
        self.stats.lock().unwrap().clone()
    }
    pub fn request(&self, id: u64, source: Source, epoch: u64, point: [i32; 2], clipboard: bool) {
        self.enqueue(id, source, epoch, point, clipboard, false, None, None);
    }
    pub fn explicit_clipboard(&self, id: u64, source: Source, epoch: u64) {
        self.enqueue(id, source, epoch, [0, 0], true, true, None, None);
    }
    pub fn copy(&self, id: u64, source: Source, epoch: u64, text: String, sequence: u32) {
        self.enqueue(
            id,
            source,
            epoch,
            [0, 0],
            true,
            true,
            Some(text),
            Some(sequence),
        );
    }
    pub fn paste(&self, id: u64, source: Source, epoch: u64, sequence: u32) {
        self.enqueue(id, source, epoch, [0, 0], true, true, None, Some(sequence));
    }
    fn enqueue(
        &self,
        id: u64,
        source: Source,
        epoch: u64,
        point: [i32; 2],
        clipboard: bool,
        explicit_clipboard: bool,
        copy: Option<String>,
        sequence: Option<u32>,
    ) {
        let mouse = if clipboard {
            None
        } else {
            super::native::mouse_anchor(source)
        };
        let old = self.pending.lock().unwrap().replace(Pending {
            query: Query {
                version: VERSION,
                nonce: self.nonce.clone(),
                id,
                source,
                epoch,
                point: mouse.unwrap_or(point),
                pointer_anchor: mouse.is_some(),
                clipboard,
                explicit_clipboard,
                write_buffer: None,
                clipboard_sequence: if clipboard {
                    Some(sequence.unwrap_or_else(|| unsafe {
                        windows::Win32::System::DataExchange::GetClipboardSequenceNumber()
                    }))
                } else {
                    None
                },
            },
            at: Instant::now(),
            copy,
        });
        if let Some(old) = old {
            emit(&self.result, Outcome::Fault(old.query.id, "superseded"));
        }
    }
    pub fn restore(&self) {
        self.restore.store(true, Ordering::Release);
    }
}
impl Drop for Coordinator {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

```

## src/platform/windows/helper.rs
```rust
//! All third-party UIA calls, including event registration/removal and callbacks,
//! live in this disposable process. Nothing here is linked into the UI thread.
use super::util;
use crate::core::protocol::{self, Query, Rect, Reply, Selection, Status};
use std::{
    io::{self, BufReader},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Com::*, DataExchange::*, Memory::*, Ole::*, Variant::*},
        UI::Accessibility::*,
    },
};

// COM may deliver callbacks on RPC threads while the MTA loop still heartbeats.
// A packed active-count/oldest-start stamp makes those hangs observable too.
static CALLBACKS: AtomicU64 = AtomicU64::new(0);
fn tick() -> u32 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount() }
}
struct Callback;
impl Callback {
    fn enter() -> Self {
        let now = tick();
        let _ = CALLBACKS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
            let count = v as u32;
            if count == u32::MAX {
                std::process::exit(71);
            }
            Some(if count == 0 {
                ((now as u64) << 32) | 1
            } else {
                v + 1
            })
        });
        Self
    }
}
impl Drop for Callback {
    fn drop(&mut self) {
        let _ = CALLBACKS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
            Some(if v as u32 <= 1 { 0 } else { v - 1 })
        });
    }
}
fn callback_watchdog() {
    std::thread::spawn(|| loop {
        let active = CALLBACKS.load(Ordering::Acquire);
        if active as u32 > 0 && tick().wrapping_sub((active >> 32) as u32) > 1000 {
            std::process::exit(72);
        }
        std::thread::sleep(Duration::from_millis(20));
    });
}
#[implement(IUIAutomationEventHandler)]
struct Events {
    dirty: Arc<AtomicBool>,
}
impl IUIAutomationEventHandler_Impl for Events_Impl {
    fn HandleAutomationEvent(
        &self,
        _sender: Ref<'_, IUIAutomationElement>,
        _event: UIA_EVENT_ID,
    ) -> Result<()> {
        let _callback = Callback::enter();
        self.dirty.store(true, Ordering::Release);
        Ok(())
    }
}
struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
struct Array(*mut SAFEARRAY);
impl Drop for Array {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = SafeArrayDestroy(self.0);
            }
        }
    }
}
unsafe fn array<T: Default + Copy>(
    raw: *mut SAFEARRAY,
    kind: VARENUM,
    max: usize,
) -> Result<Vec<T>> {
    unsafe {
        let a = Array(raw);
        if a.0.is_null()
            || SafeArrayGetDim(a.0) != 1
            || SafeArrayGetVartype(a.0)? != kind
            || SafeArrayGetElemsize(a.0) as usize != std::mem::size_of::<T>()
        {
            return Err(Error::from_hresult(HRESULT(0x80070057u32 as i32)));
        }
        let lo = SafeArrayGetLBound(a.0, 1)?;
        let hi = SafeArrayGetUBound(a.0, 1)?;
        let n = (hi as i64 - lo as i64 + 1).max(0) as usize;
        if n > max {
            return Err(Error::from_hresult(HRESULT(0x80070057u32 as i32)));
        }
        let mut out = Vec::with_capacity(n);
        for i in lo..=hi {
            let mut value = T::default();
            SafeArrayGetElement(a.0, &i, &mut value as *mut _ as *mut _)?;
            out.push(value);
        }
        Ok(out)
    }
}
unsafe fn selected(
    element: &IUIAutomationElement,
    q: &Query,
) -> std::result::Result<Selection, Status> {
    unsafe {
        if element.CurrentProcessId().map_err(|_| Status::Failed)? != q.source.pid as i32 {
            return Err(Status::SourceChanged);
        }
        if element
            .CurrentIsPassword()
            .map_err(|_| Status::Protected)?
            .as_bool()
        {
            return Err(Status::Protected);
        }
        let pattern: IUIAutomationTextPattern = element
            .GetCurrentPatternAs(UIA_TextPatternId)
            .map_err(|_| Status::Unsupported)?;
        let ranges = pattern.GetSelection().map_err(|_| Status::Failed)?;
        let count = ranges.Length().map_err(|_| Status::Failed)?;
        if count < 0 || count > 128 {
            return Err(Status::Failed);
        }
        let mut text = String::new();
        let mut rects = Vec::new();
        for i in 0..count {
            let range = ranges.GetElement(i).map_err(|_| Status::Failed)?;
            let raw = range
                .GetText((protocol::MAX_CHARS * 2 + 1) as i32)
                .map_err(|_| Status::Failed)?;
            if raw.len() > protocol::MAX_CHARS * 2 + 1 {
                return Err(Status::Unsupported);
            }
            let part = String::from_utf16(&raw).map_err(|_| Status::Failed)?;
            if part.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&part);
            if text.chars().count() > protocol::MAX_CHARS {
                return Err(Status::Failed);
            }
            let raw = range.GetBoundingRectangles().map_err(|_| Status::Failed)?;
            let values = array::<f64>(raw, VT_R8, 4096).map_err(|_| Status::Failed)?;
            if values.len() % 4 != 0 {
                return Err(Status::Failed);
            }
            for v in values.chunks_exact(4) {
                let rect = Rect {
                    x: v[0],
                    y: v[1],
                    width: v[2],
                    height: v[3],
                };
                if rect.valid() {
                    rects.push(rect);
                }
                if rects.len() > 1024 {
                    return Err(Status::Failed);
                }
            }
        }
        if text.trim().is_empty() {
            return Err(Status::Empty);
        }
        if crate::core::prompts::is_url(&text) {
            return Err(Status::Unsupported);
        }
        if q.pointer_anchor {
            if rects.is_empty() {
                rects.push(Rect {
                    x: q.point[0] as f64,
                    y: q.point[1] as f64,
                    width: 1.,
                    height: 1.,
                });
            } else if !rects.iter().any(|r| {
                q.point[0] as f64 >= r.x - 20.
                    && q.point[0] as f64 <= r.x + r.width + 20.
                    && q.point[1] as f64 >= r.y - 20.
                    && q.point[1] as f64 <= r.y + r.height + 20.
            }) {
                return Err(Status::Empty);
            }
        }
        let runtime_id = array::<i32>(
            element.GetRuntimeId().map_err(|_| Status::Failed)?,
            VT_I4,
            128,
        )
        .map_err(|_| Status::Failed)?;
        let s = Selection {
            source: q.source,
            epoch: q.epoch,
            runtime_id,
            text,
            rects,
            clipboard_sequence: None,
        };
        if !s.validate() {
            return Err(Status::Failed);
        }
        Ok(s)
    }
}
unsafe fn query(
    uia: &IUIAutomation,
    q: &Query,
) -> std::result::Result<(Selection, IUIAutomationElement), Status> {
    unsafe {
        if !util::is_current(q.source) {
            return Err(Status::SourceChanged);
        }
        let focused = uia.GetFocusedElement().map_err(|_| Status::Unsupported)?;
        if focused.CurrentProcessId().ok() == Some(q.source.pid as i32)
            && focused
                .CurrentIsPassword()
                .map_err(|_| Status::Protected)?
                .as_bool()
        {
            return Err(Status::Protected);
        }
        if q.clipboard {
            return clipboard(q).map(|s| (s, focused));
        }
        let mut candidates = vec![focused];
        if let Ok(e) = uia.ElementFromPoint(POINT {
            x: q.point[0],
            y: q.point[1],
        }) {
            candidates.push(e);
        }
        let walker = uia.ControlViewWalker().map_err(|_| Status::Unsupported)?;
        let mut last = Status::Unsupported;
        for mut element in candidates {
            if !inside_window(&element, &walker, q.source.hwnd) {
                continue;
            }
            for _ in 0..8 {
                if element.CurrentProcessId().ok() != Some(q.source.pid as i32) {
                    break;
                }
                match selected(&element, q) {
                    Ok(s) => {
                        if !util::is_current(q.source) {
                            return Err(Status::SourceChanged);
                        }
                        return Ok((s, element));
                    }
                    Err(Status::Protected) => return Err(Status::Protected),
                    Err(e) => last = e,
                }
                let Ok(parent) = walker.GetParentElement(&element) else {
                    break;
                };
                element = parent;
            }
        }
        Err(last)
    }
}
unsafe fn inside_window(
    element: &IUIAutomationElement,
    walker: &IUIAutomationTreeWalker,
    root: u64,
) -> bool {
    unsafe {
        let mut node = element.clone();
        for _ in 0..16 {
            if let Ok(h) = node.CurrentNativeWindowHandle() {
                if !h.is_invalid() {
                    return windows::Win32::UI::WindowsAndMessaging::GetAncestor(
                        h,
                        windows::Win32::UI::WindowsAndMessaging::GA_ROOT,
                    ) == util::hwnd(root);
                }
            }
            let Ok(parent) = walker.GetParentElement(&node) else {
                return false;
            };
            node = parent;
        }
        false
    }
}
unsafe fn clipboard(q: &Query) -> std::result::Result<Selection, Status> {
    unsafe {
        let before = GetClipboardSequenceNumber();
        if q.clipboard_sequence != Some(before) {
            return Err(Status::SourceChanged);
        }
        let owner = GetClipboardOwner().ok();
        // Unknown/cross-process owners are left for an explicit local preview; they
        // must never be attributed to the foreground application's live selection.
        if !q.explicit_clipboard
            && owner
                .and_then(|h| util::source(h, q.source.generation))
                .is_none_or(|s| s.pid != q.source.pid || s.created != q.source.created)
        {
            return Err(Status::SourceChanged);
        }
        OpenClipboard(None).map_err(|_| Status::Busy)?;
        struct Clipboard;
        impl Drop for Clipboard {
            fn drop(&mut self) {
                unsafe {
                    let _ = CloseClipboard();
                }
            }
        }
        let _clipboard = Clipboard;
        let handle = GetClipboardData(13).map_err(|_| Status::Empty)?;
        let global = HGLOBAL(handle.0);
        let size = GlobalSize(global);
        if size == 0 || size > (protocol::MAX_CHARS * 2 + 2) * 2 {
            return Err(Status::Failed);
        }
        let ptr = GlobalLock(global) as *const u16;
        if ptr.is_null() {
            return Err(Status::Failed);
        }
        struct Lock(HGLOBAL);
        impl Drop for Lock {
            fn drop(&mut self) {
                unsafe {
                    let _ = GlobalUnlock(self.0);
                }
            }
        }
        let _lock = Lock(global);
        let units = std::slice::from_raw_parts(ptr, size / 2);
        let end = units.iter().position(|n| *n == 0).ok_or(Status::Failed)?;
        let text = String::from_utf16(&units[..end]).map_err(|_| Status::Failed)?;
        if before != GetClipboardSequenceNumber() || owner != GetClipboardOwner().ok() {
            return Err(Status::SourceChanged);
        }
        let selection = Selection {
            source: q.source,
            epoch: q.epoch,
            runtime_id: vec![],
            text,
            rects: vec![],
            clipboard_sequence: Some(before),
        };
        if !selection.validate() {
            return Err(Status::Empty);
        }
        Ok(selection)
    }
}

unsafe fn write_clipboard(q: &Query, owner: HWND) -> std::result::Result<u32, Status> {
    unsafe {
        let buffer = q.write_buffer.as_ref().ok_or(Status::Failed)?;
        if q.clipboard_sequence != Some(GetClipboardSequenceNumber()) {
            return Err(Status::SourceChanged);
        }
        if buffer.handle == 0 || buffer.bytes == 0 || buffer.bytes > 256 * 1024 {
            return Err(Status::Failed);
        }
        let handle = util::Handle(HANDLE(buffer.handle as usize as *mut _));
        let view = MapViewOfFile(handle.0, FILE_MAP_READ, 0, 0, buffer.bytes as usize);
        if view.Value.is_null() {
            return Err(Status::Failed);
        }
        struct View(MEMORY_MAPPED_VIEW_ADDRESS);
        impl Drop for View {
            fn drop(&mut self) {
                unsafe {
                    let _ = UnmapViewOfFile(self.0);
                }
            }
        }
        let _view = View(view);
        let bytes = std::slice::from_raw_parts(view.Value as *const u8, buffer.bytes as usize);
        let text = std::str::from_utf8(bytes).map_err(|_| Status::Failed)?;
        if text.contains('\0') {
            return Err(Status::Failed);
        }
        let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let memory = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, text.len() * 2)
            .map_err(|_| Status::Failed)?;
        struct Global(HGLOBAL, bool);
        impl Drop for Global {
            fn drop(&mut self) {
                if !self.1 {
                    unsafe {
                        let _ = GlobalFree(Some(self.0));
                    }
                }
            }
        }
        let mut global = Global(memory, false);
        let ptr = GlobalLock(memory) as *mut u16;
        if ptr.is_null() {
            return Err(Status::Failed);
        }
        std::ptr::copy_nonoverlapping(text.as_ptr(), ptr, text.len());
        let _ = GlobalUnlock(memory);
        OpenClipboard(Some(owner)).map_err(|_| Status::Busy)?;
        struct Clipboard;
        impl Drop for Clipboard {
            fn drop(&mut self) {
                unsafe {
                    let _ = CloseClipboard();
                }
            }
        }
        let _clipboard = Clipboard;
        if q.clipboard_sequence != Some(GetClipboardSequenceNumber()) {
            return Err(Status::SourceChanged);
        }
        EmptyClipboard().map_err(|_| Status::Failed)?;
        SetClipboardData(13, Some(HANDLE(memory.0))).map_err(|_| Status::Failed)?;
        global.1 = true;
        Ok(GetClipboardSequenceNumber())
    }
}

pub fn run() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 2 && args[1] == "--capabilities" {
        println!("{}", super::capabilities("selection-host"));
        return;
    }
    if args.len() != 3 || args[1] != "--nonce" || args[2].len() != 48 {
        return;
    }
    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
        return;
    }
    let _com = Com;
    callback_watchdog();
    let owner = unsafe {
        windows::Win32::UI::WindowsAndMessaging::CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("BeeK clipboard owner"),
            windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            None,
        )
    }
    .ok();
    struct Owner(Option<HWND>);
    impl Drop for Owner {
        fn drop(&mut self) {
            if let Some(h) = self.0 {
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(h);
                }
            }
        }
    }
    let _owner = Owner(owner);
    let mut own_sequences = std::collections::VecDeque::new();
    let mut own_after = None;
    let mut own_id = 0;
    let mut uia: Option<IUIAutomation> = None;
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut input = BufReader::new(io::stdin());
        while let Ok(q) = protocol::read_frame::<Query>(&mut input) {
            if tx.send(q).is_err() {
                break;
            }
        }
    });
    let dirty = Arc::new(AtomicBool::new(false));
    let handler: IUIAutomationEventHandler = Events {
        dirty: dirty.clone(),
    }
    .into();
    let mut subscribed: Option<IUIAutomationElement> = None;
    let mut subscribed_window = None;
    let mut previous: Option<Query> = None;
    let mut at = Instant::now();
    let mut output = io::stdout();
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(mut q) => {
                if q.clipboard && q.explicit_clipboard {
                    let current = unsafe { GetClipboardSequenceNumber() };
                    if own_after == Some(current)
                        && q.id > own_id
                        && q.clipboard_sequence
                            .is_some_and(|s| own_sequences.contains(&s))
                    {
                        q.clipboard_sequence = Some(current);
                    } else if own_after != Some(current) {
                        own_sequences.clear();
                        own_after = None;
                    }
                }
                if q.version != protocol::VERSION || q.nonce != args[2] {
                    return;
                }
                #[cfg(feature = "test-fixtures")]
                if std::env::var("BEEK_TEST_HELPER_MODE").as_deref() == Ok("pre-ready-hang") {
                    loop {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
                let ready = Reply {
                    version: protocol::VERSION,
                    build: env!("BEEK_BUILD_ID").into(),
                    nonce: q.nonce.clone(),
                    id: q.id,
                    source: q.source,
                    selection: None,
                    status: Status::Ready,
                    clipboard_sequence: None,
                };
                if protocol::write_frame(&ready, &mut output).is_err() {
                    break;
                }
                #[cfg(feature = "test-fixtures")]
                match std::env::var("BEEK_TEST_HELPER_MODE")
                    .unwrap_or_default()
                    .as_str()
                {
                    "query-hang" => loop {
                        std::thread::sleep(Duration::from_secs(1));
                    },
                    "clipboard-hang" if q.clipboard => loop {
                        std::thread::sleep(Duration::from_secs(1));
                    },
                    "crash" => std::process::exit(73),
                    "callback-hang" => {
                        let _guard = Callback::enter();
                        loop {
                            std::thread::sleep(Duration::from_secs(1));
                        }
                    }
                    "malformed" => {
                        use std::io::Write;
                        let _ = output.write_all(b"not-json\n");
                        return;
                    }
                    "oversize" => {
                        use std::io::Write;
                        let _ = output.write_all(&vec![b'x'; protocol::MAX_FRAME + 1]);
                        return;
                    }
                    _ => {}
                }
                if q.write_buffer.is_some() {
                    if !q.clipboard || !q.explicit_clipboard {
                        return;
                    }
                    let written = owner
                        .ok_or(Status::Failed)
                        .and_then(|owner| unsafe { write_clipboard(&q, owner) });
                    if let Ok(sequence) = written {
                        if let Some(before) = q.clipboard_sequence {
                            own_sequences.push_back(before);
                        }
                        own_sequences.push_back(sequence);
                        while own_sequences.len() > 16 {
                            own_sequences.pop_front();
                        }
                        own_after = Some(sequence);
                        own_id = q.id;
                    }
                    let status = written
                        .as_ref()
                        .map(|_| Status::ClipboardWritten)
                        .unwrap_or_else(|e| *e);
                    let reply = Reply {
                        version: protocol::VERSION,
                        build: env!("BEEK_BUILD_ID").into(),
                        nonce: q.nonce.clone(),
                        id: q.id,
                        source: q.source,
                        selection: None,
                        status,
                        clipboard_sequence: written.ok(),
                    };
                    if protocol::write_frame(&reply, &mut output).is_err() {
                        break;
                    }
                    previous = Some(q);
                    at = Instant::now();
                    continue;
                }
                dirty.store(false, Ordering::Release);
                if subscribed_window != Some(q.source.hwnd) {
                    if let (Some(old), Some(uia)) = (subscribed.take(), uia.as_ref()) {
                        unsafe {
                            let _ = uia.RemoveAutomationEventHandler(
                                UIA_Text_TextSelectionChangedEventId,
                                &old,
                                &handler,
                            );
                        }
                    }
                    subscribed_window = None;
                }
                let result = if q.clipboard && q.explicit_clipboard {
                    unsafe { clipboard(&q).map(|s| (s, None)) }
                } else {
                    if uia.is_none() {
                        uia =
                            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
                                .ok();
                    }
                    match uia.as_ref() {
                        Some(uia) => unsafe {
                            // Chromium lazily materializes its accessibility tree
                            // on the first native UIA request. Bounded retries stay
                            // inside the parent's single 1500 ms query deadline.
                            let mut result = query(uia, &q);
                            for _ in 0..3 {
                                if !matches!(result, Err(Status::Unsupported)) {
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(40));
                                result = query(uia, &q);
                            }
                            result.map(|(s, e)| (s, Some(e)))
                        },
                        None => Err(Status::Failed),
                    }
                };
                let (selection, status) = match result {
                    Ok((s, e)) => {
                        if let (Some(_), Some(uia)) = (e, uia.as_ref()) {
                            if !q.clipboard && subscribed.is_none() {
                                if let Ok(root) =
                                    unsafe { uia.ElementFromHandle(util::hwnd(q.source.hwnd)) }
                                {
                                    if unsafe {
                                        uia.AddAutomationEventHandler(
                                            UIA_Text_TextSelectionChangedEventId,
                                            &root,
                                            TreeScope_Subtree,
                                            None,
                                            &handler,
                                        )
                                    }
                                    .is_ok()
                                    {
                                        subscribed = Some(root);
                                        subscribed_window = Some(q.source.hwnd);
                                    }
                                }
                            }
                        }
                        (Some(s), Status::Selected)
                    }
                    Err(e) => (None, e),
                };
                let reply = Reply {
                    version: protocol::VERSION,
                    build: env!("BEEK_BUILD_ID").into(),
                    nonce: q.nonce.clone(),
                    id: q.id,
                    source: q.source,
                    selection,
                    status,
                    clipboard_sequence: None,
                };
                if protocol::write_frame(&reply, &mut output).is_err() {
                    break;
                }
                previous = Some(q);
                at = Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(q) = &previous {
                    if !util::is_current(q.source) {
                        if let (Some(old), Some(uia)) = (subscribed.take(), uia.as_ref()) {
                            unsafe {
                                let _ = uia.RemoveAutomationEventHandler(
                                    UIA_Text_TextSelectionChangedEventId,
                                    &old,
                                    &handler,
                                );
                            }
                            subscribed_window = None;
                        }
                    }
                    let changed = dirty.swap(false, Ordering::AcqRel);
                    if changed || at.elapsed() >= Duration::from_millis(500) {
                        let reply = Reply {
                            version: protocol::VERSION,
                            build: env!("BEEK_BUILD_ID").into(),
                            nonce: q.nonce.clone(),
                            id: q.id,
                            source: q.source,
                            selection: None,
                            clipboard_sequence: None,
                            status: if changed {
                                Status::Changed
                            } else {
                                Status::Alive
                            },
                        };
                        if protocol::write_frame(&reply, &mut output).is_err() {
                            break;
                        }
                        at = Instant::now();
                    }
                }
            }
        }
    }
    if let (Some(old), Some(uia)) = (subscribed, uia.as_ref()) {
        unsafe {
            let _ = uia.RemoveAutomationEventHandler(
                UIA_Text_TextSelectionChangedEventId,
                &old,
                &handler,
            );
        }
    }
}

```

## src/platform/windows/util.rs
```rust
use crate::core::protocol::{Rect, Source};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        Security::*,
        System::{
            Com::CoTaskMemFree, RemoteDesktop::ProcessIdToSessionId, StationsAndDesktops::*,
            Threading::*,
        },
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

pub fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
pub fn nonce() -> std::result::Result<String, String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|_| "系统随机数不可用")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub struct Handle(pub HANDLE);
// Kernel handles (unlike HWND/COM pointers) may be closed/waited from any thread.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}
pub fn hwnd(value: u64) -> HWND {
    HWND(value as usize as *mut _)
}
pub fn source(window: HWND, generation: u64) -> Option<Source> {
    unsafe {
        if !IsWindow(Some(window)).as_bool() {
            return None;
        }
        let window = GetAncestor(window, GA_ROOT);
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let p = Handle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
        let (mut created, mut exit, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        GetProcessTimes(p.0, &mut created, &mut exit, &mut kernel, &mut user).ok()?;
        let created = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
        if !eligible(p.0, pid, created) {
            return None;
        }
        Some(Source {
            hwnd: window.0 as u64,
            pid,
            created,
            generation,
        })
    }
}
struct Token {
    sid: Vec<u8>,
    elevated: bool,
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
        Some(Token {
            sid,
            elevated: elevation.TokenIsElevated != 0,
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
            .is_some_and(|(own, other)| own.sid == other.sid && (!other.elevated || own.elevated));
    if cache.len() >= 64 {
        cache.clear();
    }
    cache.insert((pid, created), valid);
    valid
}
pub fn current(generation: u64) -> Option<Source> {
    if super::native::MAIN_STARTED.load(std::sync::atomic::Ordering::Acquire) {
        if !super::native::DESKTOP_ACTIVE.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
    } else if !desktop_available() {
        return None;
    }
    source(unsafe { GetForegroundWindow() }, generation)
        .filter(|s| s.pid != unsafe { GetCurrentProcessId() })
}
pub fn desktop_available() -> bool {
    unsafe {
        let Ok(input) = OpenInputDesktop(
            DESKTOP_CONTROL_FLAGS(0),
            false,
            DESKTOP_ACCESS_FLAGS(DESKTOP_SWITCHDESKTOP.0 | DESKTOP_READOBJECTS.0),
        ) else {
            return false;
        };
        struct Desktop(HDESK);
        impl Drop for Desktop {
            fn drop(&mut self) {
                unsafe {
                    let _ = CloseDesktop(self.0);
                }
            }
        }
        let _guard = Desktop(input);
        let Ok(thread) = GetThreadDesktop(GetCurrentThreadId()) else {
            return false;
        };
        fn name(h: HDESK) -> Option<Vec<u16>> {
            let mut value = [0u16; 256];
            let mut n = 0;
            unsafe {
                GetUserObjectInformationW(
                    HANDLE(h.0),
                    UOI_NAME,
                    Some(value.as_mut_ptr() as *mut _),
                    512,
                    Some(&mut n),
                )
                .ok()?;
            }
            let end = value.iter().position(|n| *n == 0)?;
            Some(value[..end].to_vec())
        }
        name(input).zip(name(thread)).is_some_and(|(a, b)| a == b)
    }
}
pub fn is_current(s: Source) -> bool {
    current(s.generation) == Some(s)
}
pub fn identity_alive(s: Source) -> bool {
    source(hwnd(s.hwnd), s.generation) == Some(s)
}
pub fn process_alive(pid: u32, created: u64) -> bool {
    unsafe {
        let Ok(h) = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            pid,
        ) else {
            return false;
        };
        let h = Handle(h);
        if WaitForSingleObject(h.0, 0) != WAIT_TIMEOUT {
            return false;
        }
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        GetProcessTimes(h.0, &mut c, &mut e, &mut k, &mut u).is_ok()
            && (((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64) == created
    }
}
pub fn known_folder(local: bool) -> std::result::Result<PathBuf, String> {
    unsafe {
        let p = SHGetKnownFolderPath(
            if local {
                &FOLDERID_LocalAppData
            } else {
                &FOLDERID_RoamingAppData
            },
            KF_FLAG_DEFAULT,
            None,
        )
        .map_err(|_| "无法定位 Windows 用户数据目录")?;
        let result = p
            .to_string()
            .map(PathBuf::from)
            .map_err(|_| "用户数据目录包含无效字符".to_owned());
        CoTaskMemFree(Some(p.0 as *const _));
        result.map(|p| p.join("BeeK"))
    }
}
pub fn cursor() -> [i32; 2] {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    [p.x, p.y]
}
pub fn work_area(point: [i32; 2]) -> Rect {
    unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: point[0],
                y: point[1],
            },
            MONITOR_DEFAULTTONEAREST,
        );
        let mut m = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut m).as_bool() {
            return Rect {
                x: 0.,
                y: 0.,
                width: 800.,
                height: 600.,
            };
        }
        Rect {
            x: m.rcWork.left as f64,
            y: m.rcWork.top as f64,
            width: (m.rcWork.right - m.rcWork.left) as f64,
            height: (m.rcWork.bottom - m.rcWork.top) as f64,
        }
    }
}
pub fn fatal(message: &str) {
    let m = wide(message);
    unsafe {
        MessageBoxW(None, PCWSTR(m.as_ptr()), w!("BeeK"), MB_OK | MB_ICONERROR);
    }
}
pub fn check_platform() -> std::result::Result<(), String> {
    if !cfg!(target_arch = "x86_64") {
        return Err("首版 BeeK 需要 Windows 11 x64".into());
    }
    if token(unsafe { GetCurrentProcess() }).is_none_or(|t| t.elevated) {
        return Err("请以普通用户权限启动 BeeK，无需管理员权限".into());
    }
    use windows::Win32::System::SystemInformation::OSVERSIONINFOEXW;
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(info: *mut OSVERSIONINFOEXW) -> i32;
    }
    let mut version = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOEXW>() as u32,
        ..Default::default()
    };
    if unsafe { RtlGetVersion(&mut version) } < 0
        || version.dwMajorVersion < 10
        || version.dwBuildNumber < 22000
        || version.wProductType != 1
    {
        return Err("BeeK 首版需要 Windows 11 x64 桌面系统".into());
    }
    Ok(())
}

```
