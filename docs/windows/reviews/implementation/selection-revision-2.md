# Revision 2: selection

Selection.M1：保留“一次在途恢复探测，第四次实际失败隔离”，新增Health::abandon，在所有管理性取消/无源启动失败/非故障非成功终态释放探测占用并冷却1秒；没有改成永久周期自旋。取消的Starting/running busy查询均发Fault终态。unit canceled_probe_can_recover_but_failed_probe_stays_quarantined通过；真实Windows协调器先三次1500ms超时、30秒后第四探测被SYSTEM_EPOCH取消，再发第五次正常请求Selected精确公开fixture，403ms；不计故障注入为真实UIA兼容性。m1/m3新增TooLong，不把长度超限冒充Empty/Failed，不回收健康worker。m4 stderr改独立继承NUL，不进JSON协议。另将ElementFromPoint改focused失败后才惰性调用并对Unsupported最多3*40ms重试，整体1500ms不变：新Chrome进程100/100匹配。新增RestartChild在本process模块，但它不是UIA/helper槽：原子临时候选Job，ACK后handoff去掉kill-on-close并设SILENT_BREAKAWAY；请核对不会影响原UIA上下限。

以下只列与首轮快照的真实差异，未出现区域未修改。允许Read补读本附件。G5、正式发布均未通过。

## src/core/protocol.rs
```diff
--- round1/src/core/protocol.rs
+++ current/src/core/protocol.rs
@@ -118,8 +118,9 @@
     Ready,
     Selected,
     Empty,
     Unsupported,
+    TooLong,
     Protected,
     SourceChanged,
     Busy,
     Failed,

```

## src/core/policy.rs
```diff
--- round1/src/core/policy.rs
+++ current/src/core/policy.rs
@@ -40,8 +40,18 @@
             }
         }
         true
     }
+    /// Administrative cancellation is not a provider failure and cannot consume
+    /// the sole recovery probe forever. Keep at most one probe in flight.
+    pub fn abandon(&mut self, source: Source, now: u64) {
+        if let Some(f) = self.sources.get_mut(&(source.pid, source.created)) {
+            if f.failures == 3 && !f.quarantined && f.probe_used {
+                f.probe_used = false;
+                f.next = now.saturating_add(1000);
+            }
+        }
+    }
     pub fn attempt(&mut self, now: u64) -> bool {
         while self
             .attempts
             .front()
@@ -191,5 +201,22 @@
         assert!(h.attempt(0));
         h.restore();
         assert!(h.begin(src(1), 0));
     }
-}
+    #[test]
+    fn canceled_probe_can_recover_but_failed_probe_stays_quarantined() {
+        let mut h = Health::default();
+        let s = src(9);
+        for now in [0, 1000, 6000] {
+            h.failure(Some(s), now, false);
+        }
+        assert!(h.begin(s, 36000));
+        assert!(!h.begin(s, 36001));
+        h.abandon(s, 36010);
+        assert!(!h.begin(s, 37009));
+        assert!(h.begin(s, 37010));
+        assert!(!h.begin(s, 37011));
+        h.failure(Some(s), 37012, false);
+        h.abandon(s, 37013);
+        assert!(!h.begin(s, 1_000_000));
+    }
+}

```

## src/platform/windows/process.rs
```diff
--- round1/src/platform/windows/process.rs
+++ current/src/platform/windows/process.rs
@@ -17,8 +17,9 @@
     core::{BOOL, PCWSTR, PWSTR},
     Win32::{
         Foundation::*,
         Security::SECURITY_ATTRIBUTES,
+        Storage::FileSystem::*,
         System::{JobObjects::*, Memory::*, Pipes::*, Threading::*},
     },
 };
 
@@ -52,8 +53,9 @@
 pub struct Child {
     pub query: Query,
     pub start: Instant,
     pub process: Handle,
+    pid: u32,
     pub job: Arc<Jobs>,
     pub reader: Option<thread::JoinHandle<()>>,
     pub result: mpsc::Receiver<Result<Reply, String>>,
     pub input: File,
@@ -106,9 +108,21 @@
                     }
                 }
             }
             let _attributes = Attributes(list);
-            let handles = [stdin_child.0, stdout_child.0];
+            let stderr = Handle(
+                CreateFileW(
+                    windows::core::w!("NUL"),
+                    GENERIC_WRITE.0,
+                    FILE_SHARE_READ | FILE_SHARE_WRITE,
+                    Some(&attrs),
+                    OPEN_EXISTING,
+                    FILE_ATTRIBUTE_NORMAL,
+                    None,
+                )
+                .map_err(|_| "无法隔离助手标准错误")?,
+            );
+            let handles = [stdin_child.0, stdout_child.0, stderr.0];
             let jobs = [main_job.0 .0, job.0 .0];
             UpdateProcThreadAttribute(
                 list,
                 0,
@@ -133,9 +147,9 @@
             startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
             startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
             startup.StartupInfo.hStdInput = stdin_child.0;
             startup.StartupInfo.hStdOutput = stdout_child.0;
-            startup.StartupInfo.hStdError = stdout_child.0;
+            startup.StartupInfo.hStdError = stderr.0;
             startup.lpAttributeList = list;
             let app = util::wide(&helper);
             let command = format!("\"{}\" --nonce {}", helper.display(), query.nonce);
             let mut command = util::wide(command);
@@ -166,8 +180,25 @@
             super::diagnostics::event(
                 "worker_spawned",
                 serde_json::json!({"pid":info.dwProcessId,"source_pid":query.source.pid,"clipboard":query.clipboard,"atomic_job":true}),
             );
+            struct Registration(u32, bool);
+            impl Drop for Registration {
+                fn drop(&mut self) {
+                    if self.1 {
+                        let _ = super::native::CLIPBOARD_HELPER_PID.compare_exchange(
+                            self.0,
+                            0,
+                            Ordering::AcqRel,
+                            Ordering::Acquire,
+                        );
+                    }
+                }
+            }
+            let mut registration = Registration(info.dwProcessId, query.clipboard);
+            if query.clipboard {
+                super::native::CLIPBOARD_HELPER_PID.store(info.dwProcessId, Ordering::Release);
+            }
             if ResumeThread(thread.0) == u32::MAX {
                 let _ = TerminateJobObject(job.0 .0, 1);
                 return Err("助手线程启动失败".into());
             }
@@ -195,9 +226,11 @@
                         break;
                     }
                 }
             });
+            registration.1 = false;
             Ok(Self {
+                pid: info.dwProcessId,
                 query,
                 start,
                 process,
                 job,
@@ -234,8 +267,16 @@
     }
 }
 impl Drop for Child {
     fn drop(&mut self) {
+        if self.query.clipboard {
+            let _ = super::native::CLIPBOARD_HELPER_PID.compare_exchange(
+                self.pid,
+                0,
+                Ordering::AcqRel,
+                Ordering::Acquire,
+            );
+        }
         self.kill();
     }
 }
 fn attach_copy(query: &mut Query, text: Option<&str>, process: HANDLE) -> Result<(), String> {
@@ -280,4 +321,128 @@
         });
     }
     Ok(())
 }
+
+// Restart candidates have their own atomic job, separate from disposable UIA
+// helpers. A crashed parent cannot orphan a candidate during GPU preflight.
+pub struct RestartChild {
+    pub process: Handle,
+    pub pid: u32,
+    job: Arc<Jobs>,
+    committed: bool,
+}
+impl RestartChild {
+    pub fn launch(
+        exe: &std::path::Path,
+        arguments: &str,
+        job: Arc<Jobs>,
+        cancel: &AtomicBool,
+    ) -> Result<Self, String> {
+        unsafe {
+            if cancel.load(Ordering::Acquire) {
+                return Err("重启创建已取消".into());
+            }
+            let mut bytes = 0;
+            let _ = InitializeProcThreadAttributeList(None, 1, None, &mut bytes);
+            let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
+            let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr() as *mut _);
+            InitializeProcThreadAttributeList(Some(list), 1, None, &mut bytes)
+                .map_err(|_| "重启属性初始化失败")?;
+            struct Attr(LPPROC_THREAD_ATTRIBUTE_LIST);
+            impl Drop for Attr {
+                fn drop(&mut self) {
+                    unsafe {
+                        DeleteProcThreadAttributeList(self.0);
+                    }
+                }
+            }
+            let _attrs = Attr(list);
+            let jobs = [job.0 .0];
+            UpdateProcThreadAttribute(
+                list,
+                0,
+                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
+                Some(jobs.as_ptr() as *const _),
+                std::mem::size_of_val(&jobs),
+                None,
+                None,
+            )
+            .map_err(|_| "重启原子作业隔离失败")?;
+            let app = util::wide(exe);
+            let mut command = util::wide(format!("\"{}\" {arguments}", exe.display()));
+            let mut startup = STARTUPINFOEXW::default();
+            startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
+            startup.lpAttributeList = list;
+            let mut info = PROCESS_INFORMATION::default();
+            CreateProcessW(
+                PCWSTR(app.as_ptr()),
+                Some(PWSTR(command.as_mut_ptr())),
+                None,
+                None,
+                false,
+                CREATE_NO_WINDOW | CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
+                None,
+                None,
+                &startup.StartupInfo,
+                &mut info,
+            )
+            .map_err(|_| "无法创建重启子进程，旧实例继续运行")?;
+            let child = Self {
+                process: Handle(info.hProcess),
+                pid: info.dwProcessId,
+                job,
+                committed: false,
+            };
+            let thread = Handle(info.hThread);
+            if cancel.load(Ordering::Acquire) {
+                return Err("重启创建已取消，旧实例继续运行".into());
+            }
+            if ResumeThread(thread.0) == u32::MAX {
+                return Err("无法运行重启子进程".into());
+            }
+            Ok(child)
+        }
+    }
+    pub fn commit(&mut self) -> Result<(), String> {
+        unsafe {
+            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
+            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
+            SetInformationJobObject(
+                self.job.0 .0,
+                JobObjectExtendedLimitInformation,
+                &info as *const _ as *const _,
+                std::mem::size_of_val(&info) as u32,
+            )
+            .map_err(|_| "无法交接重启子进程")?;
+        }
+        self.committed = true;
+        Ok(())
+    }
+}
+impl Drop for RestartChild {
+    fn drop(&mut self) {
+        if !self.committed {
+            self.job.kill();
+            unsafe {
+                let _ = WaitForSingleObject(self.process.0, INFINITE);
+            }
+        }
+    }
+}
+pub fn restart_job() -> Result<Arc<Jobs>, String> {
+    let job = Jobs::new(1)?;
+    unsafe {
+        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
+        info.BasicLimitInformation.LimitFlags =
+            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
+        info.BasicLimitInformation.ActiveProcessLimit = 1;
+        SetInformationJobObject(
+            job.0 .0,
+            JobObjectExtendedLimitInformation,
+            &info as *const _ as *const _,
+            std::mem::size_of_val(&info) as u32,
+        )
+        .map_err(|_| "无法创建重启隔离作业")?;
+    }
+    Ok(job)
+}

```

## src/platform/windows/selection.rs
```diff
--- round1/src/platform/windows/selection.rs
+++ current/src/platform/windows/selection.rs
@@ -94,8 +94,45 @@
     }
     queue.push_back(outcome);
 }
 
+fn cancel_worker(
+    w: &mut Worker,
+    health: &mut Health,
+    result: &Arc<Mutex<VecDeque<Outcome>>>,
+    now: u64,
+    code: &'static str,
+) {
+    if !w.terminating {
+        if w.busy.is_some() {
+            emit(result, Outcome::Fault(w.child.query.id, code));
+        }
+        if !w.child.query.clipboard {
+            health.abandon(w.child.query.source, now);
+        }
+        w.child.kill();
+        w.terminating = true;
+    }
+}
+fn cancel_slot(
+    slot: &mut Slot,
+    health: &mut Health,
+    result: &Arc<Mutex<VecDeque<Outcome>>>,
+    now: u64,
+    code: &'static str,
+) {
+    match slot {
+        Slot::Running(w) => cancel_worker(w, health, result, now, code),
+        Slot::Starting(s) if !s.terminating => {
+            emit(result, Outcome::Fault(s.query.id, code));
+            if !s.query.clipboard {
+                health.abandon(s.query.source, now);
+            }
+            slot.kill();
+        }
+        _ => {}
+    }
+}
 fn failure(health: &mut Health, query: &Query, handshaken: bool, now: u64, protocol_fault: bool) {
     super::diagnostics::event(
         "worker_fault",
         serde_json::json!({"id":query.id,"source_pid":query.source.pid,"clipboard":query.clipboard,"handshaken":handshaken,"protocol_fault":protocol_fault}),
@@ -103,8 +140,11 @@
     // A clipboard renderer is not attributed to the foreground UIA provider.
     // All process creation attempts still consume the one shared rolling budget.
     if query.clipboard && handshaken {
         return;
+    }
+    if !query.clipboard && !handshaken {
+        health.abandon(query.source, now);
     }
     health.failure(
         if handshaken { Some(query.source) } else { None },
         now,
@@ -201,9 +241,15 @@
                     && w.child.query.write_buffer.is_none())
                 || selected != reply.selection.is_some()
                 || (reply.status == Status::ClipboardWritten) != reply.clipboard_sequence.is_some()
                 || reply.selection.as_ref().is_some_and(|s| {
-                    !s.validate() || s.source != reply.source || s.epoch != w.child.query.epoch
+                    !s.validate()
+                        || s.source != reply.source
+                        || s.epoch != w.child.query.epoch
+                        || s.clipboard_sequence.is_some() != w.child.query.clipboard
+                        || (w.child.query.clipboard
+                            && !w.child.query.explicit_clipboard
+                            && s.clipboard_sequence != w.child.query.clipboard_sequence)
                 })
             {
                 stop_worker(w, health, result, now, "worker_protocol", true);
                 break;
@@ -239,10 +285,13 @@
                             Status::Selected
                                 | Status::Empty
                                 | Status::Unsupported
                                 | Status::Protected
+                                | Status::TooLong
                         ) {
                             health.success(reply.source);
+                        } else {
+                            health.abandon(reply.source, now);
                         }
                     }
                     super::diagnostics::event(
                         "selection_reply",
@@ -264,10 +313,9 @@
         if !w.terminating && w.child.exited() {
             stop_worker(w, health, result, now, "worker_crash", true);
         }
         if !w.terminating && !util::identity_alive(w.child.query.source) {
-            w.child.kill();
-            w.terminating = true;
+            cancel_worker(w, health, result, now, "source_gone");
         }
         if w.terminating && w.child.reaped() {
             *slot = Slot::Gone;
         }
@@ -330,15 +378,20 @@
             let mut last_prune = Instant::now();
             let mut reported_pause = false;
             let mut system_epoch = super::native::SYSTEM_EPOCH.load(Ordering::Acquire);
             while !s.load(Ordering::Acquire) {
+                let now = clock.elapsed().as_millis() as u64;
                 let context = super::native::SYSTEM_EPOCH.load(Ordering::Acquire);
                 if context != system_epoch {
                     system_epoch = context;
-                    waiting = None;
-                    let _ = p.lock().unwrap().take();
+                    if let Some(old) = waiting.take() {
+                        emit(&r, Outcome::Fault(old.query.id, "system_context_changed"));
+                    }
+                    if let Some(old) = p.lock().unwrap().take() {
+                        emit(&r, Outcome::Fault(old.query.id, "system_context_changed"));
+                    }
                     for slot in &mut slots {
-                        slot.kill();
+                        cancel_slot(slot, &mut health, &r, now, "system_context_changed");
                     }
                 }
                 if reset.swap(false, Ordering::AcqRel) {
                     health.restore();
@@ -348,9 +401,8 @@
                     if let Some(old) = waiting.replace(new) {
                         emit(&r, Outcome::Fault(old.query.id, "superseded"));
                     }
                 }
-                let now = clock.elapsed().as_millis() as u64;
                 for slot in &mut slots {
                     poll_slot(slot, &mut health, &r, now);
                 }
                 slots.retain(|s| !matches!(s, Slot::Gone));
@@ -368,9 +420,9 @@
                         waiting = None;
                     }
                     for slot in &mut slots {
                         if slot.query().is_some_and(|q| !q.clipboard) {
-                            slot.kill();
+                            cancel_slot(slot, &mut health, &r, now, "platform_paused");
                         }
                     }
                 } else {
                     reported_pause = false;
@@ -429,9 +481,15 @@
                                     old.source != q.query.source || old.epoch != q.query.epoch
                                 }
                             });
                             if stale && !slots[index].terminating() {
-                                slots[index].kill();
+                                cancel_slot(
+                                    &mut slots[index],
+                                    &mut health,
+                                    &r,
+                                    now,
+                                    "superseded_context",
+                                );
                             }
                         } else {
                             let count = slots
                                 .iter()
@@ -444,11 +502,16 @@
                                         continue;
                                     }
                                     let q = waiting.take().unwrap();
                                     let id = q.query.id;
+                                    let source = q.query.source;
+                                    let is_clipboard = q.query.clipboard;
                                     match launch(q, jobs.clone()) {
                                         Ok(slot) => slots.push(slot),
                                         Err(_) => {
+                                            if !is_clipboard {
+                                                health.abandon(source, now);
+                                            }
                                             health.failure(None, now, true);
                                             emit(&r, Outcome::Fault(id, "worker_start"));
                                         }
                                     }
@@ -470,9 +533,9 @@
                                         }) && !s.terminating()
                                     })
                                     .min_by_key(|s| s.oldest())
                                 {
-                                    slot.kill();
+                                    cancel_slot(slot, &mut health, &r, now, "preempted_context");
                                 }
                             }
                         }
                     }

```

## src/platform/windows/helper.rs
```diff
--- round1/src/platform/windows/helper.rs
+++ current/src/platform/windows/helper.rs
@@ -73,8 +73,17 @@
         self.dirty.store(true, Ordering::Release);
         Ok(())
     }
 }
+fn phase(q: &Query, stage: &'static str) {
+    #[cfg(feature = "test-fixtures")]
+    super::diagnostics::event(
+        "provider_phase",
+        serde_json::json!({"pid":std::process::id(),"source_pid":q.source.pid,"id":q.id,"stage":stage}),
+    );
+    #[cfg(not(feature = "test-fixtures"))]
+    let _ = (q, stage);
+}
 struct Com;
 impl Drop for Com {
     fn drop(&mut self) {
         unsafe {
@@ -150,10 +159,10 @@
             let range = ranges.GetElement(i).map_err(|_| Status::Failed)?;
             let raw = range
                 .GetText((protocol::MAX_CHARS * 2 + 1) as i32)
                 .map_err(|_| Status::Failed)?;
-            if raw.len() > protocol::MAX_CHARS * 2 + 1 {
-                return Err(Status::Unsupported);
+            if raw.len() > protocol::MAX_CHARS * 2 {
+                return Err(Status::TooLong);
             }
             let part = String::from_utf16(&raw).map_err(|_| Status::Failed)?;
             if part.is_empty() {
                 continue;
@@ -162,9 +171,9 @@
                 text.push('\n');
             }
             text.push_str(&part);
             if text.chars().count() > protocol::MAX_CHARS {
-                return Err(Status::Failed);
+                return Err(Status::TooLong);
             }
             let raw = range.GetBoundingRectangles().map_err(|_| Status::Failed)?;
             let values = array::<f64>(raw, VT_R8, 4096).map_err(|_| Status::Failed)?;
             if values.len() % 4 != 0 {
@@ -222,9 +231,9 @@
             rects,
             clipboard_sequence: None,
         };
         if !s.validate() {
-            return Err(Status::Failed);
+            return Err(Status::Unsupported);
         }
         Ok(s)
     }
 }
@@ -235,8 +244,9 @@
     unsafe {
         if !util::is_current(q.source) {
             return Err(Status::SourceChanged);
         }
+        phase(q, "focused");
         let focused = uia.GetFocusedElement().map_err(|_| Status::Unsupported)?;
         if focused.CurrentProcessId().ok() == Some(q.source.pid as i32)
             && focused
                 .CurrentIsPassword()
@@ -247,25 +257,34 @@
         }
         if q.clipboard {
             return clipboard(q).map(|s| (s, focused));
         }
-        let mut candidates = vec![focused];
-        if let Ok(e) = uia.ElementFromPoint(POINT {
-            x: q.point[0],
-            y: q.point[1],
-        }) {
-            candidates.push(e);
-        }
+        // Hit-testing can itself block a provider. Only use it when the focused
+        // element/ancestors cannot supply a valid selected range.
+        let candidates = std::iter::once(focused).chain(
+            std::iter::once_with(|| {
+                phase(q, "point");
+                uia.ElementFromPoint(POINT {
+                    x: q.point[0],
+                    y: q.point[1],
+                })
+                .ok()
+            })
+            .flatten(),
+        );
+        phase(q, "walker");
         let walker = uia.ControlViewWalker().map_err(|_| Status::Unsupported)?;
         let mut last = Status::Unsupported;
         for mut element in candidates {
+            phase(q, "ancestry");
             if !inside_window(&element, &walker, q.source.hwnd) {
                 continue;
             }
             for _ in 0..8 {
                 if element.CurrentProcessId().ok() != Some(q.source.pid as i32) {
                     break;
                 }
+                phase(q, "selected");
                 match selected(&element, q) {
                     Ok(s) => {
                         if !util::is_current(q.source) {
                             return Err(Status::SourceChanged);
@@ -336,9 +355,12 @@
         let _clipboard = Clipboard;
         let handle = GetClipboardData(13).map_err(|_| Status::Empty)?;
         let global = HGLOBAL(handle.0);
         let size = GlobalSize(global);
-        if size == 0 || size > (protocol::MAX_CHARS * 2 + 2) * 2 {
+        if size > (protocol::MAX_CHARS * 2 + 2) * 2 {
+            return Err(Status::TooLong);
+        }
+        if size < 2 || size % 2 != 0 {
             return Err(Status::Failed);
         }
         let ptr = GlobalLock(global) as *const u16;
         if ptr.is_null() {
@@ -366,8 +388,11 @@
             text,
             rects: vec![],
             clipboard_sequence: Some(before),
         };
+        if selection.text.chars().count() > protocol::MAX_CHARS {
+            return Err(Status::TooLong);
+        }
         if !selection.validate() {
             return Err(Status::Empty);
         }
         Ok(selection)
@@ -454,8 +479,10 @@
     if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
         return;
     }
     let _com = Com;
+    #[cfg(feature = "test-fixtures")]
+    super::diagnostics::initialize();
     callback_watchdog();
     let owner = unsafe {
         windows::Win32::UI::WindowsAndMessaging::CreateWindowExW(
             windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),

```

## src/platform/windows/util.rs
```diff
--- round1/src/platform/windows/util.rs
+++ current/src/platform/windows/util.rs
@@ -286,9 +286,22 @@
     unsafe {
         MessageBoxW(None, PCWSTR(m.as_ptr()), w!("BeeK"), MB_OK | MB_ICONERROR);
     }
 }
+pub fn native_machine() -> Option<u16> {
+    let mut process = windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE::default();
+    let mut native = process;
+    unsafe {
+        IsWow64Process2(GetCurrentProcess(), &mut process, Some(&mut native)).ok()?;
+    }
+    Some(native.0)
+}
 pub fn check_platform() -> std::result::Result<(), String> {
+    if native_machine()
+        != Some(windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_AMD64.0)
+    {
+        return Err("首版仅支持原生 Windows 11 x64，不支持 ARM64 仿真".into());
+    }
     if !cfg!(target_arch = "x86_64") {
         return Err("首版 BeeK 需要 Windows 11 x64".into());
     }
     if token(unsafe { GetCurrentProcess() }).is_none_or(|t| t.elevated) {

```
