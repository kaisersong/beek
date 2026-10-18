# Revision 2: storage-lifecycle

Storage.B1：原产品UI已有startup_settings：load后若api_key非空立即事务Replace，未随首轮附件提供。为使Backend契约自身防误用，transact的Keep也会将无ref的legacy key升级为Replace，新增Keep迁移成功/commit失败保原key回归测试。M1 child等待父提交改read_for(6000)，公共IPC每操作仍1000。M2 validate新增URL4096、hotkey64、ref128、legacykey2560上限，任何可提交的自身配置远低于load1MiB。损坏备份改内容散列去重，保留至少最近3份，其他旧备份24h后清理；管道连续创建失败50次发元数据诊断和UI错误并降频1s。自查另修：CreateProcess移出UI线程，真实创建槽lease在实际创建返回/子进程退出前不释放，候选重启进程原子Job归属防父崩溃孤儿；ChildReady/ParentExitCommitted/ChildCommitAccepted三步，只有Root消费Committed才允许自身退出的handoff，否则用户Quit会杀候选Job。子进程在父实际死亡后重读已提交配置，避免预检快照与并发Save不一致；失败候选不弹独立阻塞对话框。附当前Native RestartChild及startup_settings调用点，均属本轮新增需核查。

以下只列与首轮快照的真实差异，未出现区域未修改。允许Read补读本附件。G5、正式发布均未通过。

## src/core/settings.rs
```diff
--- round1/src/core/settings.rs
+++ current/src/core/settings.rs
@@ -50,8 +50,19 @@
     pub fn validate(&self) -> Result<(), String> {
         if self.schema != 1 {
             return Err("不支持的配置版本，请使用相应版本 BeeK".into());
         }
+        if self.provider.base_url.len() > 4096
+            || self.hotkey.len() > 64
+            || self
+                .provider
+                .credential_ref
+                .as_ref()
+                .is_some_and(|r| r.is_empty() || r.len() > 128)
+            || self.provider.api_key.len() > 2560
+        {
+            return Err("配置字段超出长度限制，原配置已保留".into());
+        }
         if self.provider.model.trim().is_empty() || self.provider.model.len() > 1024 {
             return Err("模型名称无效".into());
         }
         let url = reqwest::Url::parse(&self.provider.base_url).map_err(|_| "API 地址无效")?;
@@ -84,8 +95,16 @@
     mut next: Settings,
     edit: KeyEdit,
 ) -> Result<Settings, String> {
     next.validate()?;
+    let edit = match edit {
+        KeyEdit::Keep
+            if old.provider.credential_ref.is_none() && !old.provider.api_key.is_empty() =>
+        {
+            KeyEdit::Replace(old.provider.api_key.clone())
+        }
+        edit => edit,
+    };
     let staged = match edit {
         KeyEdit::Keep => {
             next.provider.credential_ref = old.provider.credential_ref.clone();
             None
@@ -200,5 +219,37 @@
         let s: Settings = serde_json::from_str("{}").unwrap();
         assert!(!s.copy_to_popup && !s.autostart);
         s.validate().unwrap();
     }
-}
+    #[test]
+    fn keeping_a_legacy_key_migrates_transactionally_instead_of_erasing_it() {
+        let mut old = Settings::default();
+        old.provider.api_key = "public-legacy-fixture".into();
+        let mut memory = Memory {
+            fail: true,
+            ..Default::default()
+        };
+        assert!(transact(&mut memory, &old, old.clone(), KeyEdit::Keep).is_err());
+        assert_eq!(old.provider.api_key, "public-legacy-fixture");
+        assert!(memory.secrets.is_empty());
+        memory.fail = false;
+        let saved = transact(&mut memory, &old, old.clone(), KeyEdit::Keep).unwrap();
+        assert_eq!(
+            memory
+                .read_secret(saved.provider.credential_ref.as_ref().unwrap())
+                .unwrap(),
+            "public-legacy-fixture"
+        );
+        assert!(!serde_json::to_string(&saved)
+            .unwrap()
+            .contains("public-legacy-fixture"));
+    }
+    #[test]
+    fn accepted_configuration_fields_are_bounded_for_reload() {
+        let mut s = Settings::default();
+        s.provider.base_url = format!("https://example.invalid/{}", "x".repeat(4096));
+        assert!(s.validate().is_err());
+        s.provider.base_url = Provider::default().base_url;
+        s.hotkey = "x".repeat(65);
+        assert!(s.validate().is_err());
+    }
+}

```

## src/platform/windows/store.rs
```diff
--- round1/src/platform/windows/store.rs
+++ current/src/platform/windows/store.rs
@@ -52,18 +52,64 @@
             .map_err(|error| self.preserve_invalid(&error))?;
         Ok(cfg)
     }
     fn preserve_invalid(&self, reason: &str) -> String {
-        let backup =
-            util::nonce().map(|nonce| self.path.with_extension(format!("corrupt-{nonce}.json")));
+        let backup = self
+            .hash()
+            .and_then(|hash| hash.ok_or("配置已消失".into()))
+            .map(|hash| {
+                self.path
+                    .with_extension(format!("corrupt-{:x}.json", Sha256::digest(hash)))
+            });
         if backup
-            .and_then(|path| fs::copy(&self.path, path).map_err(|_| "备份失败".into()))
+            .and_then(|path| {
+                if path.exists() {
+                    Ok(0)
+                } else {
+                    fs::copy(&self.path, path).map_err(|_| "备份失败".into())
+                }
+            })
             .is_err()
         {
             *self.expected.lock().unwrap() = None;
             format!("{reason}；无法备份原文件，请手动恢复后重新启动")
         } else {
+            self.prune_backups();
             format!("{reason}；原文件已备份，保存恢复配置前请确认设置")
+        }
+    }
+    fn prune_backups(&self) {
+        if let Some(dir) = self.path.parent() {
+            if let Ok(entries) = fs::read_dir(dir) {
+                let mut backups = entries
+                    .flatten()
+                    .filter(|e| {
+                        let name = e.file_name();
+                        let n = name.to_string_lossy();
+                        n.strip_prefix("config.corrupt-")
+                            .and_then(|n| n.strip_suffix(".json"))
+                            .is_some_and(|n| {
+                                matches!(n.len(), 48 | 64)
+                                    && n.bytes().all(|b| b.is_ascii_hexdigit())
+                            })
+                    })
+                    .filter_map(|e| {
+                        e.metadata()
+                            .ok()
+                            .filter(|m| m.is_file())
+                            .and_then(|m| m.modified().ok())
+                            .map(|t| (t, e.path()))
+                    })
+                    .collect::<Vec<_>>();
+                backups.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
+                for (t, path) in backups.into_iter().skip(3) {
+                    if t.elapsed()
+                        .is_ok_and(|age| age >= std::time::Duration::from_secs(24 * 3600))
+                    {
+                        let _ = fs::remove_file(path);
+                    }
+                }
+            }
         }
     }
     pub fn save(
         &mut self,

```

## src/platform/windows/lifecycle.rs
```diff
--- round1/src/platform/windows/lifecycle.rs
+++ current/src/platform/windows/lifecycle.rs
@@ -199,8 +199,11 @@
             complete(self.0 .0, &mut ov, status, timeout).map(|_| ())
         }
     }
     fn read<T: for<'a> Deserialize<'a>>(&self) -> Result<T, String> {
+        self.read_for(1000)
+    }
+    fn read_for<T: for<'a> Deserialize<'a>>(&self, timeout: u32) -> Result<T, String> {
         unsafe {
             let event =
                 Handle(CreateEventW(None, true, false, None).map_err(|_| "IPC 事件不可用")?);
             let mut ov = OVERLAPPED {
@@ -208,9 +211,9 @@
                 ..Default::default()
             };
             let mut bytes = [0u8; 4097];
             let status = ReadFile(self.0 .0, Some(&mut bytes), None, Some(&mut ov));
-            let n = complete(self.0 .0, &mut ov, status, 1000)?;
+            let n = complete(self.0 .0, &mut ov, status, timeout)?;
             if n == 0 || n > 4096 {
                 return Err("IPC 消息长度无效".into());
             }
             serde_json::from_slice(&bytes[..n]).map_err(|_| "IPC 消息格式无效".into())
@@ -308,8 +311,12 @@
     stop: Arc<AtomicBool>,
     listener: Option<thread::JoinHandle<()>>,
     pub restart_events: Option<mpsc::Receiver<RestartEvent>>,
     grant: Option<mpsc::SyncSender<()>>,
+    restart_busy: Arc<AtomicBool>,
+    restart_cancel: Option<Arc<AtomicBool>>,
+    restart_job: Option<Arc<super::process::Jobs>>,
+    restart_committed: bool,
     sid: String,
     identity: Identity,
 }
 impl Instance {
@@ -389,16 +396,32 @@
         let shutdown = stop.clone();
         let thread_sid = sid.clone();
         let listener = thread::spawn(move || {
             let mut next = Some(first);
+            let mut create_failures = 0u32;
             while !shutdown.load(Ordering::Acquire) {
                 let pipe = match next
                     .take()
                     .or_else(|| Pipe::server(&pipe_name, &thread_sid).ok())
                 {
-                    Some(p) => p,
+                    Some(p) => {
+                        create_failures = 0;
+                        p
+                    }
                     None => {
-                        thread::sleep(Duration::from_millis(20));
+                        create_failures = create_failures.saturating_add(1);
+                        if create_failures == 50 {
+                            super::diagnostics::event(
+                                "ipc_unavailable",
+                                serde_json::json!({"consecutive_failures":create_failures}),
+                            );
+                            let _ = events.try_send(Event::IpcUnavailable);
+                        }
+                        thread::sleep(Duration::from_millis(if create_failures >= 50 {
+                            1000
+                        } else {
+                            20
+                        }));
                         continue;
                     }
                 };
                 if pipe.connect(250).is_err() {
@@ -440,14 +463,18 @@
             stop,
             listener: Some(listener),
             restart_events: None,
             grant: None,
+            restart_busy: Arc::new(AtomicBool::new(false)),
+            restart_cancel: None,
+            restart_job: None,
+            restart_committed: false,
             sid,
             identity: own,
         }))
     }
     pub fn restart(&mut self) -> Result<(), String> {
-        if self.restart_events.is_some() {
+        if self.restart_events.is_some() || self.restart_busy.load(Ordering::Acquire) {
             return Err("重启正在进行".into());
         }
         let nonce = util::nonce()?;
         let name = format!(
@@ -455,22 +482,57 @@
             self.sid, self.identity.session
         );
         let pipe = Pipe::server(&name, &self.sid)?;
         let exe = std::env::current_exe().map_err(|_| "程序路径不可用")?;
-        let mut child = std::process::Command::new(exe)
-            .args([
-                "--restart-child",
-                &self.identity.pid.to_string(),
-                &self.identity.created.to_string(),
-                &nonce,
-            ])
-            .spawn()
-            .map_err(|_| "无法启动重启子进程，旧进程继续运行")?;
-        let child_pid = child.id();
+        let job = super::process::restart_job()?;
+        let cancel = Arc::new(AtomicBool::new(false));
+        self.restart_cancel = Some(cancel.clone());
+        self.restart_job = Some(job.clone());
+        self.restart_committed = false;
+        self.restart_busy.store(true, Ordering::Release);
+        struct Busy(Arc<AtomicBool>);
+        impl Drop for Busy {
+            fn drop(&mut self) {
+                self.0.store(false, Ordering::Release);
+            }
+        }
+        let busy = Arc::new(Busy(self.restart_busy.clone()));
         let (tx, rx) = mpsc::sync_channel(2);
         let (grant_tx, grant_rx) = mpsc::sync_channel(1);
         let parent = self.identity;
         thread::spawn(move || {
+            let _busy = busy;
+            let (created, creation) = mpsc::sync_channel(1);
+            let spawn_job = job.clone();
+            let spawn_cancel = cancel.clone();
+            let spawn_busy = _busy.clone();
+            let arguments = format!("--restart-child {} {} {nonce}", parent.pid, parent.created);
+            thread::spawn(move || {
+                let _busy = spawn_busy;
+                let result = super::process::RestartChild::launch(
+                    &exe,
+                    &arguments,
+                    spawn_job,
+                    &spawn_cancel,
+                );
+                let _ = created.send(result);
+            });
+            let mut child = match creation.recv_timeout(Duration::from_secs(5)) {
+                Ok(Ok(child)) => child,
+                Ok(Err(error)) => {
+                    let _ = tx.send(RestartEvent::Failed(error));
+                    return;
+                }
+                Err(_) => {
+                    cancel.store(true, Ordering::Release);
+                    job.kill();
+                    let _ = tx.send(RestartEvent::Failed(
+                        "重启创建超时，旧实例继续运行；创建槽实际释放前不能重复重试".into(),
+                    ));
+                    return;
+                }
+            };
+            let child_pid = child.pid;
             let result = (|| -> Result<(), String> {
                 pipe.connect(5000)?;
                 let message: RestartMessage = pipe.read()?;
                 let actual = pipe.peer(false)?;
@@ -500,29 +562,39 @@
                     || accepted.state != RestartState::ChildCommitAccepted
                 {
                     return Err("重启提交未确认，旧进程继续运行".into());
                 }
+                child.commit()?;
                 let _ = tx.send(RestartEvent::Committed);
                 Ok(())
             })();
             if let Err(e) = result {
-                let _ = child.kill();
-                let _ = child.wait();
                 let _ = tx.send(RestartEvent::Failed(e));
             }
         });
         self.restart_events = Some(rx);
         self.grant = Some(grant_tx);
         Ok(())
     }
+    pub fn commit_exit(&mut self) {
+        self.restart_committed = true;
+    }
     pub fn grant_restart(&mut self) {
         if let Some(tx) = self.grant.take() {
             let _ = tx.try_send(());
         }
     }
 }
 impl Drop for Instance {
     fn drop(&mut self) {
+        if !self.restart_committed {
+            if let Some(cancel) = &self.restart_cancel {
+                cancel.store(true, Ordering::Release);
+            }
+            if let Some(job) = &self.restart_job {
+                job.kill();
+            }
+        }
         self.stop.store(true, Ordering::Release);
         let at = Instant::now();
         while self.listener.as_ref().is_some_and(|t| !t.is_finished())
             && at.elapsed() < Duration::from_millis(1100)
@@ -598,9 +670,9 @@
         nonce: args[4].clone(),
         process: own,
         state: RestartState::ChildReady,
     })?;
-    let response: RestartMessage = pipe.read()?;
+    let response: RestartMessage = pipe.read_for(6000)?;
     if response.version != 1
         || response.nonce != args[4]
         || response.process != parent
         || response.state != RestartState::ParentExitCommitted

```

## 当前完整RestartChild与Job交接
```rust
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
    let job = Jobs::new(1)?;
    unsafe {
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        info.BasicLimitInformation.ActiveProcessLimit = 1;
        SetInformationJobObject(
            job.0 .0,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of_val(&info) as u32,
        )
        .map_err(|_| "无法创建重启隔离作业")?;
    }
    Ok(job)
}

```

## 当前startup_settings与启动/重启配置调用
```rust
fn startup_settings(store: &mut Store) -> Result<Settings, String> {
    let cfg = store.load()?;
    if cfg.provider.api_key.is_empty() {
        Ok(cfg)
    } else {
        let key = cfg.provider.api_key.clone();
        store.save(&cfg, cfg.clone(), KeyEdit::Replace(key))
    }
}
pub fn run() -> Result<(), String> {
    util::check_platform()?;
    let _installation = super::lifecycle::install_guard()?;
    let restarting = std::env::args().nth(1).as_deref() == Some("--restart-child");
    let (etx, erx) = mpsc::sync_channel(32);
    let instance = if restarting {
        None
    } else {
        match super::lifecycle::Instance::begin(etx.clone())? {
            Some(i) => Some(i),
            None => return Ok(()),
        }
    };
    super::pair::helper()?;
    let store = Arc::new(Mutex::new(Store::open()?));
    let loaded = {
        let mut store = store.lock().unwrap();
        let loaded = startup_settings(&mut store);
        if loaded.is_ok() {
            let _ = store.cleanup();
        }
        loaded
    };
    let mut valid = loaded.is_ok();
    let mut initial_error = loaded.as_ref().err().cloned().unwrap_or_default();
    let initial = loaded.unwrap_or_default();
    let pool = Pool::new();
    let (tx, rx) = mpsc::sync_channel(32);
    let (bg_tx, bg_rx) = mpsc::sync_channel(2);
    let (work_tx, work_rx) = mpsc::sync_channel(1);
    let mut cfg = initial;
    let startup_store = store.clone();
    std::thread::spawn(move || {
        while let Ok(work) = work_rx.recv() {
            let result = match work {
                Work::Save(old, next, key) => {
                    let saved = {
                        let mut store = store.lock().unwrap();
                        let saved = store.save(&old, next, key);
                        if saved.is_ok() {
                            let _ = store.cleanup();
                        }
                        saved
                    };
                    let warning = saved
                        .as_ref()
                        .ok()
                        .and_then(|cfg| super::integration::autostart(cfg.autostart).err());
                    Background::Saved(saved, warning)
                }
                Work::Request(g, config, text) => {
                    let key = store.lock().unwrap().key(&config);
                    Background::Request(
                        g,
                        key.map(|key| stream::Request {
                            generation: g,
                            text,
                            key,
                            model: config.provider.model,
                            endpoint: config.provider.base_url,
                        }),
                    )
                }
            };
            if bg_tx.send(result).is_err() {
                break;
            }
        }
    });
    Application::new().run(move|cx|{
  input::bind(cx);cx.bind_keys([KeyBinding::new("tab",NextField,Some("BeeKSettings")),KeyBinding::new("shift-tab",PreviousField,Some("BeeKSettings")),KeyBinding::new("ctrl-s",SaveFields,Some("BeeKSettings")),KeyBinding::new("enter",TranslateSnapshot,Some("BeeKPreview")),KeyBinding::new("escape",CloseView,Some("BeeKPreview")),KeyBinding::new("ctrl-c",CopyPreview,Some("BeeKPreview"))]);
  let host=match cx.open_window(WindowOptions{titlebar:None,show:false,focus:false,kind:WindowKind::PopUp,window_bounds:Some(WindowBounds::Windowed(Bounds::new(point(px(0.),px(0.)),size(px(1.),px(1.))))),..Default::default()},|_,cx|cx.new(|_|Host)).map_err(|_|()){Ok(host)=>host,Err(_)=>{util::fatal("图形初始化失败，请检查显卡驱动");cx.quit();return;}};
  // A restart child reaches GPUI/GPU window creation before ChildReady. Until
  // ParentExitCommitted and actual parent death it installs no tray or hooks.
  let initialized=(||->Result<_,String>{
   let instance=if restarting{super::lifecycle::child_preflight()?;let committed=startup_settings(&mut startup_store.lock().unwrap());valid=committed.is_ok();initial_error=committed.as_ref().err().cloned().unwrap_or_default();cfg=committed.unwrap_or_default();super::lifecycle::Instance::begin(etx.clone())?.ok_or("重启后会话锁仍被占用")?}else{instance.unwrap()};
   let coordinator=Coordinator::start()?;let desktop=native::Desktop::start(etx)?;desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);Ok((instance,coordinator,desktop))
  })();
  let(mut instance,coordinator,desktop)=match initialized{Ok(v)=>v,Err(e)=>{if !restarting{util::fatal(&e);}cx.quit();return;}};
  let mut form=if std::env::args().any(|a|a=="--autostart"){None}else{open_form(cx,tx.clone(),cfg.clone(),initial_error.clone())};let mut bee:Option<WindowHandle<Bee>>=None;let mut panel:Option<WindowHandle<Panel>>=None;

```
