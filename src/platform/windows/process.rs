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
pub struct Child {
    pub query: Query,
    pub start: Instant,
    pub process: Handle,
    pid: u32,
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
            let stderr = Handle(
                CreateFileW(
                    windows::core::w!("NUL"),
                    GENERIC_WRITE.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    Some(&attrs),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
                .map_err(|_| "无法隔离助手标准错误")?,
            );
            let handles = [stdin_child.0, stdout_child.0, stderr.0];
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
            startup.StartupInfo.hStdError = stderr.0;
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
            struct Registration(u32, bool);
            impl Drop for Registration {
                fn drop(&mut self) {
                    if self.1 {
                        let _ = super::native::CLIPBOARD_HELPER_PID.compare_exchange(
                            self.0,
                            0,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                    }
                }
            }
            let mut registration = Registration(info.dwProcessId, query.clipboard);
            if query.clipboard {
                super::native::CLIPBOARD_HELPER_PID.store(info.dwProcessId, Ordering::Release);
            }
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
            registration.1 = false;
            Ok(Self {
                pid: info.dwProcessId,
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
        if self.query.clipboard {
            let _ = super::native::CLIPBOARD_HELPER_PID.compare_exchange(
                self.pid,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
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

// Restart candidates have their own atomic job, separate from disposable UIA
// helpers. A crashed parent cannot orphan a candidate during GPU preflight.
pub struct RestartChild {
    pub process: Handle,
    pub pid: u32,
    job: Arc<Jobs>,
    committed: bool,
}
impl RestartChild {
    pub fn launch(
        exe: &std::path::Path,
        arguments: &str,
        job: Arc<Jobs>,
        cancel: &AtomicBool,
    ) -> Result<Self, String> {
        unsafe {
            if cancel.load(Ordering::Acquire) {
                return Err("重启创建已取消".into());
            }
            let mut bytes = 0;
            let _ = InitializeProcThreadAttributeList(None, 1, None, &mut bytes);
            let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
            let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr() as *mut _);
            InitializeProcThreadAttributeList(Some(list), 1, None, &mut bytes)
                .map_err(|_| "重启属性初始化失败")?;
            struct Attr(LPPROC_THREAD_ATTRIBUTE_LIST);
            impl Drop for Attr {
                fn drop(&mut self) {
                    unsafe {
                        DeleteProcThreadAttributeList(self.0);
                    }
                }
            }
            let _attrs = Attr(list);
            let jobs = [job.0 .0];
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                Some(jobs.as_ptr() as *const _),
                std::mem::size_of_val(&jobs),
                None,
                None,
            )
            .map_err(|_| "重启原子作业隔离失败")?;
            let app = util::wide(exe);
            let mut command = util::wide(format!("\"{}\" {arguments}", exe.display()));
            let mut startup = STARTUPINFOEXW::default();
            startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            startup.lpAttributeList = list;
            let mut info = PROCESS_INFORMATION::default();
            CreateProcessW(
                PCWSTR(app.as_ptr()),
                Some(PWSTR(command.as_mut_ptr())),
                None,
                None,
                false,
                CREATE_NO_WINDOW | CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
                None,
                None,
                &startup.StartupInfo,
                &mut info,
            )
            .map_err(|_| "无法创建重启子进程，旧实例继续运行")?;
            let child = Self {
                process: Handle(info.hProcess),
                pid: info.dwProcessId,
                job,
                committed: false,
            };
            let thread = Handle(info.hThread);
            if cancel.load(Ordering::Acquire) {
                return Err("重启创建已取消，旧实例继续运行".into());
            }
            if ResumeThread(thread.0) == u32::MAX {
                return Err("无法运行重启子进程".into());
            }
            Ok(child)
        }
    }
    pub fn commit(&mut self) -> Result<(), String> {
        unsafe {
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
            SetInformationJobObject(
                self.job.0 .0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of_val(&info) as u32,
            )
            .map_err(|_| "无法交接重启子进程")?;
        }
        self.committed = true;
        Ok(())
    }
}
impl Drop for RestartChild {
    fn drop(&mut self) {
        if !self.committed {
            self.job.kill();
            unsafe {
                let _ = WaitForSingleObject(self.process.0, INFINITE);
            }
        }
    }
}
pub fn restart_job() -> Result<Arc<Jobs>, String> {
    // A whole GUI candidate is not subject to the disposable helper memory cap.
    Jobs::limited(1, None)
}
