# Revision 2: ui-input

UI.M1 写Clipboard helper的PID在ResumeThread前发布、实际进程Drop时CAS清除；WM_CLIPBOARDUPDATE只做廉价owner PID检查，忽略自有helper，不用易吞外部复制的时间窗口。原自动读取还会校验owner PID/created与前台来源，旧版实际上也不会把helper文本冒充来源；这次进一步消除回声查询。M2 paste_value改Option<new_revision>，与cut共用队列及同帧迟到intent重绑；重复Ctrl+V不因首个提交丢失。M3 panel Copy失败/无来源均显式反馈。m1 Verify不匹配隐藏bee；m2关闭旧form取消队列并保留反馈，不向新form应用；m3按实际应用返回报告，包括已复制但未剪切；m4系统上下文复位反馈取消；m5保留上次Save结果至重开，并用提交时字段revision/flags避免异步Save清空后来新输入；m6 UTF16区间统一钳制；m7 IME未提交时不应用异步Paste；m8 window创建失败返回Option/诊断，主GPU宿主失败显示native错误并退出，buildid切片用get。另补：正常快照/设置用原生SetForegroundWindow/SetFocus激活，不依赖框架模拟Alt；保存时有未完成编辑则拒绝。请核查这些改动及未改区域的稳定语义，不假设未附依赖缺失即产品不存在。

以下只列与首轮快照的真实差异，未出现区域未修改。允许Read补读本附件。G5、正式发布均未通过。

## src/platform/windows/ui.rs
```diff
--- round1/src/platform/windows/ui.rs
+++ current/src/platform/windows/ui.rs
@@ -7,9 +7,9 @@
 };
 use crate::{
     core::{
         policy::TTL_MS,
-        protocol::{Rect, Selection, Source},
+        protocol::{Rect, Selection, Source, Status},
         settings::{KeyEdit, Settings},
         stream::{self, Mailbox, Pool, Progress},
     },
     icons::IconKind,
@@ -260,16 +260,29 @@
     }
 }
 struct Form {
     _subscriptions: Vec<Subscription>,
+    save_revision: Option<(Vec<u64>, [bool; 5])>,
     fields: Vec<Entity<TextInput>>,
     cfg: Settings,
     status: String,
     busy: bool,
     clear: bool,
     tx: Tx,
 }
 impl Form {
+    fn signature(&self, cx: &App) -> (Vec<u64>, [bool; 5]) {
+        (
+            self.fields.iter().map(|f| f.read(cx).revision()).collect(),
+            [
+                self.cfg.selection_enabled,
+                self.cfg.copy_to_popup,
+                self.cfg.hotkey_enabled,
+                self.cfg.autostart,
+                self.clear,
+            ],
+        )
+    }
     fn next(&mut self, _: &NextField, w: &mut Window, cx: &mut Context<Self>) {
         let n = self
             .fields
             .iter()
@@ -317,8 +330,9 @@
             KeyEdit::Replace(key)
         };
         if self.tx.try_send(Command::Save(cfg, edit)).is_ok() {
             self.busy = true;
+            self.save_revision = Some(self.signature(cx));
             self.status = "正在保存…".into();
             cx.notify();
         }
     }
@@ -422,9 +436,9 @@
                 .child("仅点击翻译后发送所选文本。剪贴板回退不会模拟 Ctrl+C；不自动下载安装更新。"),
         )
     }
 }
-fn open_form(cx: &mut App, tx: Tx, cfg: Settings, status: String) -> WindowHandle<Form> {
+fn open_form(cx: &mut App, tx: Tx, cfg: Settings, status: String) -> Option<WindowHandle<Form>> {
     let handle = cx
         .open_window(
             WindowOptions {
                 titlebar: Some(TitlebarOptions {
@@ -465,16 +479,24 @@
                         .iter()
                         .map(|field| {
                             cx.subscribe(
                                 field,
-                                |this: &mut Form, field, event: &ClipboardIntent, _| {
-                                    let _ = this.tx.try_send(Command::Edit(field, event.clone()));
+                                |this: &mut Form, field, event: &ClipboardIntent, cx| {
+                                    if this
+                                        .tx
+                                        .try_send(Command::Edit(field, event.clone()))
+                                        .is_err()
+                                    {
+                                        this.status = "操作队列繁忙，请稍后重试".into();
+                                        cx.notify();
+                                    }
                                 },
                             )
                         })
                         .collect();
                     Form {
                         _subscriptions: subscriptions,
+                        save_revision: None,
                         fields,
                         cfg,
                         status,
                         busy: false,
@@ -483,13 +505,17 @@
                     }
                 })
             },
         )
-        .expect("settings window");
+        .map_err(|_| {
+            super::diagnostics::event("window_open_failed", serde_json::json!({"kind":"settings"}));
+            util::fatal("无法创建设置窗口，请检查显卡驱动后重试");
+        })
+        .ok()?;
     let _ = handle.update(cx, |_, w, _| native::activate(w));
-    handle
-}
-fn open_bee(cx: &mut App, tx: Tx, s: Selection) -> WindowHandle<Bee> {
+    Some(handle)
+}
+fn open_bee(cx: &mut App, tx: Tx, s: Selection) -> Option<WindowHandle<Bee>> {
     let anchor = s.rects.last().copied().unwrap_or(Rect {
         x: util::cursor()[0] as f64,
         y: util::cursor()[1] as f64,
         width: 1.,
@@ -518,15 +544,21 @@
                     loading: false,
                 })
             },
         )
-        .expect("selection window");
+        .map_err(|_| {
+            super::diagnostics::event(
+                "window_open_failed",
+                serde_json::json!({"kind":"selection"}),
+            )
+        })
+        .ok()?;
     let _ = w.update(cx, |_, window, _| {
         native::show_popup(window, anchor, [40., 40.])
     });
-    w
-}
-fn open_panel(cx: &mut App, tx: Tx, s: Selection, snapshot: bool) -> WindowHandle<Panel> {
+    Some(w)
+}
+fn open_panel(cx: &mut App, tx: Tx, s: Selection, snapshot: bool) -> Option<WindowHandle<Panel>> {
     let anchor = s.rects.last().copied().unwrap_or(Rect {
         x: util::cursor()[0] as f64,
         y: util::cursor()[1] as f64,
         width: 1.,
@@ -583,17 +615,23 @@
                     }
                 })
             },
         )
-        .expect("translation window");
+        .map_err(|_| {
+            super::diagnostics::event(
+                "window_open_failed",
+                serde_json::json!({"kind":"translation"}),
+            )
+        })
+        .ok()?;
     if snapshot {
         let _ = w.update(cx, |_, window, _| native::activate(window));
     } else {
         let _ = w.update(cx, |_, window, _| {
             native::show_popup(window, anchor, [520., 420.])
         });
     }
-    w
+    Some(w)
 }
 enum Purpose {
     Detect,
     Verify(Selection),
@@ -608,14 +646,33 @@
 enum Work {
     Save(Settings, Settings, KeyEdit),
     Request(u64, Settings, String),
 }
+fn selection_status(status: Status) -> &'static str {
+    match status {
+        Status::TooLong => "选区或剪贴板文本超过 16384 字符，请缩小范围",
+        Status::Protected => "该输入区域受保护，未读取文本",
+        Status::Empty => "没有可用的文本选区或纯文本剪贴板内容",
+        Status::SourceChanged => "来源或剪贴板已变化，请重新选择或重试",
+        Status::Busy => "选区检测繁忙，请稍后重试",
+        _ => "当前应用未提供可用选区，请使用托盘的剪贴板预览",
+    }
+}
 fn hide<T: 'static>(handle: &mut Option<WindowHandle<T>>, cx: &mut App) {
     if let Some(w) = handle.take() {
         let _ = w.update(cx, |_, w, _| w.remove_window());
     }
 }
 
+fn startup_settings(store: &mut Store) -> Result<Settings, String> {
+    let cfg = store.load()?;
+    if cfg.provider.api_key.is_empty() {
+        Ok(cfg)
+    } else {
+        let key = cfg.provider.api_key.clone();
+        store.save(&cfg, cfg.clone(), KeyEdit::Replace(key))
+    }
+}
 pub fn run() -> Result<(), String> {
     util::check_platform()?;
     let _installation = super::lifecycle::install_guard()?;
     let restarting = std::env::args().nth(1).as_deref() == Some("--restart-child");
@@ -631,29 +688,23 @@
     super::pair::helper()?;
     let store = Arc::new(Mutex::new(Store::open()?));
     let loaded = {
         let mut store = store.lock().unwrap();
-        let loaded = store.load().and_then(|cfg| {
-            if cfg.provider.api_key.is_empty() {
-                Ok(cfg)
-            } else {
-                let key = cfg.provider.api_key.clone();
-                store.save(&cfg, cfg.clone(), KeyEdit::Replace(key))
-            }
-        });
+        let loaded = startup_settings(&mut store);
         if loaded.is_ok() {
             let _ = store.cleanup();
         }
         loaded
     };
-    let valid = loaded.is_ok();
-    let initial_error = loaded.as_ref().err().cloned().unwrap_or_default();
+    let mut valid = loaded.is_ok();
+    let mut initial_error = loaded.as_ref().err().cloned().unwrap_or_default();
     let initial = loaded.unwrap_or_default();
     let pool = Pool::new();
     let (tx, rx) = mpsc::sync_channel(32);
     let (bg_tx, bg_rx) = mpsc::sync_channel(2);
     let (work_tx, work_rx) = mpsc::sync_channel(1);
     let mut cfg = initial;
+    let startup_store = store.clone();
     std::thread::spawn(move || {
         while let Ok(work) = work_rx.recv() {
             let result = match work {
                 Work::Save(old, next, key) => {
@@ -691,28 +742,29 @@
         }
     });
     Application::new().run(move|cx|{
   input::bind(cx);cx.bind_keys([KeyBinding::new("tab",NextField,Some("BeeKSettings")),KeyBinding::new("shift-tab",PreviousField,Some("BeeKSettings")),KeyBinding::new("ctrl-s",SaveFields,Some("BeeKSettings")),KeyBinding::new("enter",TranslateSnapshot,Some("BeeKPreview")),KeyBinding::new("escape",CloseView,Some("BeeKPreview")),KeyBinding::new("ctrl-c",CopyPreview,Some("BeeKPreview"))]);
-  let host=cx.open_window(WindowOptions{titlebar:None,show:false,focus:false,kind:WindowKind::PopUp,window_bounds:Some(WindowBounds::Windowed(Bounds::new(point(px(0.),px(0.)),size(px(1.),px(1.))))),..Default::default()},|_,cx|cx.new(|_|Host)).expect("lifetime host");
+  let host=match cx.open_window(WindowOptions{titlebar:None,show:false,focus:false,kind:WindowKind::PopUp,window_bounds:Some(WindowBounds::Windowed(Bounds::new(point(px(0.),px(0.)),size(px(1.),px(1.))))),..Default::default()},|_,cx|cx.new(|_|Host)).map_err(|_|()){Ok(host)=>host,Err(_)=>{util::fatal("图形初始化失败，请检查显卡驱动");cx.quit();return;}};
   // A restart child reaches GPUI/GPU window creation before ChildReady. Until
   // ParentExitCommitted and actual parent death it installs no tray or hooks.
   let initialized=(||->Result<_,String>{
-   let instance=if restarting{super::lifecycle::child_preflight()?;super::lifecycle::Instance::begin(etx.clone())?.ok_or("重启后会话锁仍被占用")?}else{instance.unwrap()};
+   let instance=if restarting{super::lifecycle::child_preflight()?;let committed=startup_settings(&mut startup_store.lock().unwrap());valid=committed.is_ok();initial_error=committed.as_ref().err().cloned().unwrap_or_default();cfg=committed.unwrap_or_default();super::lifecycle::Instance::begin(etx.clone())?.ok_or("重启后会话锁仍被占用")?}else{instance.unwrap()};
    let coordinator=Coordinator::start()?;let desktop=native::Desktop::start(etx)?;desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);Ok((instance,coordinator,desktop))
   })();
-  let(mut instance,coordinator,desktop)=match initialized{Ok(v)=>v,Err(e)=>{util::fatal(&e);cx.quit();return;}};
-  let mut form=if std::env::args().any(|a|a=="--autostart"){None}else{Some(open_form(cx,tx.clone(),cfg.clone(),initial_error.clone()))};let mut bee:Option<WindowHandle<Bee>>=None;let mut panel:Option<WindowHandle<Panel>>=None;
+  let(mut instance,coordinator,desktop)=match initialized{Ok(v)=>v,Err(e)=>{if !restarting{util::fatal(&e);}cx.quit();return;}};
+  let mut form=if std::env::args().any(|a|a=="--autostart"){None}else{open_form(cx,tx.clone(),cfg.clone(),initial_error.clone())};let mut bee:Option<WindowHandle<Bee>>=None;let mut panel:Option<WindowHandle<Panel>>=None;
   cx.spawn(async move|cx|{
-   let(_host,_desktop)=(host,desktop);let mut config_valid=valid;let mut epoch=native::EPOCH.load(Ordering::Acquire);let mut trigger=0;let mut id=0;let mut generation=0;let mut pending:Option<(u64,Purpose)>=None;let mut cache:Option<(Selection,Instant)>=None;let mut last_source:Option<Source>=None;let mut mailbox:Option<(u64,Mailbox)>=None;let mut dragging:Option<([i32;2],[i32;2])>=None;let mut saving=false;let mut request_snapshot=false;let mut request_epoch=0;let mut hotkey_requested=false;let mut copy_requested=false;let mut quiet_at=Instant::now();let mut system_epoch=native::SYSTEM_EPOCH.load(Ordering::Acquire);let mut edits:VecDeque<(Entity<TextInput>,ClipboardIntent)>=VecDeque::new();let mut edit_active:Option<(u64,Entity<TextInput>,ClipboardIntent)>=None;let mut failed_copy:Option<Instant>=None;let mut last_cut:Option<(Entity<TextInput>,u64,u64,Instant)>=None;
+   let(_host,_desktop)=(host,desktop);let mut config_valid=valid;let mut epoch=native::EPOCH.load(Ordering::Acquire);let mut trigger=0;let mut id=0;let mut generation=0;let mut pending:Option<(u64,Purpose)>=None;let mut cache:Option<(Selection,Instant)>=None;let mut last_source:Option<Source>=None;let mut mailbox:Option<(u64,Mailbox)>=None;let mut dragging:Option<([i32;2],[i32;2])>=None;let mut saving=false;let mut request_snapshot=false;let mut request_epoch=0;let mut hotkey_requested=false;let mut copy_requested=false;let mut quiet_at=Instant::now();let mut system_epoch=native::SYSTEM_EPOCH.load(Ordering::Acquire);let mut edits:VecDeque<(Entity<TextInput>,ClipboardIntent)>=VecDeque::new();let mut edit_active:Option<(u64,Entity<TextInput>,ClipboardIntent)>=None;let mut failed_copy:Option<Instant>=None;let mut last_applied_edit:Option<(Entity<TextInput>,u64,u64,Instant)>=None;let mut last_settings_status:Option<String>=None;
    loop {
-    let context=native::SYSTEM_EPOCH.load(Ordering::Acquire);if context!=system_epoch{system_epoch=context;edits.clear();edit_active=None;failed_copy=None;last_cut=None;pending=None;cache=None;generation+=1;pool.cancel_pending();mailbox=None;request_snapshot=false;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);if let Some(f)=form{let _=f.update(cx,|_,w,_|native::keep_accessible(w));}});}
+    let context=native::SYSTEM_EPOCH.load(Ordering::Acquire);if context!=system_epoch{system_epoch=context;edits.clear();edit_active=None;failed_copy=None;last_applied_edit=None;pending=None;cache=None;generation+=1;pool.cancel_pending();mailbox=None;request_snapshot=false;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);if let Some(f)=form{let _=f.update(cx,|f,w,cx|{let _=native::keep_accessible(w);f.status="系统状态已变化，未完成的剪贴板操作已取消".into();cx.notify();});}});}
     let changed=native::EPOCH.load(Ordering::Acquire);
     if let Some(source)=util::current(changed){last_source=Some(source);}
     if changed!=epoch{epoch=changed;quiet_at=Instant::now();pending=None;cache=None;if !request_snapshot{generation+=1;pool.cancel_pending();mailbox=None;}let _=cx.update(|cx|{hide(&mut bee,cx);if panel.is_some_and(|w|w.update(cx,|p,_,_|!p.snapshot).unwrap_or(false)){hide(&mut panel,cx);}});}
     if cache.as_ref().is_some_and(|(s,at)|at.elapsed()>Duration::from_millis(TTL_MS)||!util::is_current(s.source)){cache=None;let _=cx.update(|cx|hide(&mut bee,cx));}
     let t=native::triggered();if edit_active.is_none()&&edits.is_empty()&&cfg.selection_enabled&&!hotkey_requested&&!copy_requested&&pending.as_ref().is_none_or(|(_,p)|matches!(p,Purpose::Detect))&&t!=trigger&&native::quiet_since(t){trigger=t;if let Some(source)=util::current(epoch){id+=1;coordinator.request(id,source,epoch,util::cursor(),false);pending=Some((id,Purpose::Detect));}}
     while let Ok(event)=erx.try_recv(){match event{
-     Event::Settings|Event::About=>{let about=matches!(event,Event::About);let info=format!("BeeK {} · Windows x64 · helper {} · {}",env!("CARGO_PKG_VERSION"),&env!("BEEK_BUILD_ID")[..12],coordinator.snapshot());let _=cx.update(|cx|{hide(&mut bee,cx);let active=form.and_then(|f|f.update(cx,|_,w,_|native::activate(w)).ok());if active.is_none(){form=Some(open_form(cx,tx.clone(),cfg.clone(),if about{info.clone()}else{String::new()}));}if about{if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=info;cx.notify();});}}});},
+     Event::Settings|Event::About=>{let about=matches!(event,Event::About);let info=format!("BeeK {} · Windows x64 · helper {} · {}",env!("CARGO_PKG_VERSION"),env!("BEEK_BUILD_ID").get(..12).unwrap_or("unknown"),coordinator.snapshot());let _=cx.update(|cx|{hide(&mut bee,cx);let active=form.and_then(|f|f.update(cx,|_,w,_|native::activate(w)).ok());if active.is_none(){form=open_form(cx,tx.clone(),cfg.clone(),if about{info.clone()}else{last_settings_status.clone().unwrap_or_default()});}if about{if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=info;cx.notify();});}}});},
+     Event::IpcUnavailable=>{let message="实例通信暂不可用，请从托盘退出后重新启动".to_owned();last_settings_status=Some(message.clone());let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});});},
      Event::Restore=>coordinator.restore(),Event::Quit=>{super::lifecycle::exit_deadline();let _=cx.update(|cx|cx.quit());return;},
      Event::Restart=>{if let Err(e)=instance.restart(){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|s,_,cx|{s.status=e;cx.notify();});});}},
      Event::Hotkey=>{hotkey_requested=true;quiet_at=Instant::now();},
      Event::ClipboardChanged=>{copy_requested=true;quiet_at=Instant::now();},
@@ -724,68 +776,73 @@
     if copy_requested&&quiet_at.elapsed()>=Duration::from_millis(150){copy_requested=false;if let Some(source)=util::current(epoch){id+=1;coordinator.request(id,source,epoch,util::cursor(),true);pending=Some((id,Purpose::Clipboard(true)));}}
     let restart=instance.restart_events.as_ref().and_then(|rx|rx.try_recv().ok());
     if let Some(event)=restart{match event{
      super::lifecycle::RestartEvent::Ready=>instance.grant_restart(),
-     super::lifecycle::RestartEvent::Committed=>{super::lifecycle::exit_deadline();let _=cx.update(|cx|cx.quit());return;},
-     super::lifecycle::RestartEvent::Failed(e)=>{instance.restart_events=None;let _=cx.update(|cx|{if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=e;cx.notify();});}else{form=Some(open_form(cx,tx.clone(),cfg.clone(),e));}});}
+     super::lifecycle::RestartEvent::Committed=>{instance.commit_exit();super::lifecycle::exit_deadline();let _=cx.update(|cx|cx.quit());return;},
+     super::lifecycle::RestartEvent::Failed(e)=>{instance.restart_events=None;let _=cx.update(|cx|{if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=e;cx.notify();});}else{form=open_form(cx,tx.clone(),cfg.clone(),e);}});}
     }}
     let outcomes={coordinator.result.lock().unwrap().drain(..).collect::<Vec<_>>()};
     for outcome in outcomes{
      let reply=match &outcome{Outcome::Selected(id,_)|Outcome::Empty(id,_)|Outcome::Fault(id,_)|Outcome::Copied(id,_)=>Some(*id),_=>None};
      if edit_active.as_ref().is_some_and(|(id,_,_)|Some(*id)==reply){
       let (_,field,intent)=edit_active.take().unwrap();
-      let success=matches!((&intent,&outcome),(ClipboardIntent::Copy{..},Outcome::Copied(..))|(ClipboardIntent::Paste{..},Outcome::Selected(..)));
+      let mut applied=matches!((&intent,&outcome),(ClipboardIntent::Copy{cut:false,..},Outcome::Copied(..)));
       if let Some(f)=form{let _=f.update(cx,|f,w,cx|{
-       match (&intent,&outcome){
-        (ClipboardIntent::Paste{ticket,..},Outcome::Selected(_,selection))=>field.update(cx,|field,cx|field.paste_value(*ticket,&selection.text,w,cx)),
-        (ClipboardIntent::Copy{ticket,range,cut:true,..},Outcome::Copied(..))=>{if let Some(revision)=field.update(cx,|field,cx|field.cut_complete(*ticket,range.clone(),w,cx)){last_cut=Some((field.clone(),*ticket,revision,Instant::now()));for (target,intent) in &mut edits{if target==&field{if let ClipboardIntent::Paste{ticket:next,..}=intent{if *next==*ticket{*next=revision;}}}}}},_=>{}
+       if f.fields.contains(&field){
+        let updated=match (&intent,&outcome){
+         (ClipboardIntent::Paste{ticket,..},Outcome::Selected(_,selection))=>field.update(cx,|field,cx|field.paste_value(*ticket,&selection.text,w,cx)).map(|revision|(*ticket,revision)),
+         (ClipboardIntent::Copy{ticket,range,cut:true,..},Outcome::Copied(..))=>field.update(cx,|field,cx|field.cut_complete(*ticket,range.clone(),w,cx)).map(|revision|(*ticket,revision)),_=>None
+        };
+        if let Some((old,revision))=updated{applied=true;last_applied_edit=Some((field.clone(),old,revision,Instant::now()));for(target,intent)in &mut edits{if target==&field{if let ClipboardIntent::Paste{ticket,..}=intent{if *ticket==old{*ticket=revision;}}}}}
        }
-       f.status=if success{"剪贴板操作已完成"}else{"剪贴板操作未完成，内容保持不变，请重试"}.into();cx.notify();
+       f.status=if applied{"剪贴板操作已完成"}else if matches!(outcome,Outcome::Copied(..)){"已复制；输入位置已变化，未执行剪切"}else{"输入位置、组合文字或剪贴板已变化，操作未完成，请重试"}.into();cx.notify();
       });}
-      if !success{if let ClipboardIntent::Copy{..}=intent{failed_copy=Some(Instant::now());edits.retain(|(_,e)|!matches!(e,ClipboardIntent::Paste{..}));}}
+      #[cfg(feature="test-fixtures")]super::diagnostics::event("editor_result",serde_json::json!({"id":reply,"applied":applied}));
+      if !matches!(outcome,Outcome::Copied(..)){if let ClipboardIntent::Copy{..}=intent{failed_copy=Some(Instant::now());edits.retain(|(_,e)|!matches!(e,ClipboardIntent::Paste{..}));}}
       continue;
      }
      match outcome{
      Outcome::Selected(reply,s) if pending.as_ref().is_some_and(|(expected,_)|*expected==reply)=>{
       let purpose=pending.take().unwrap().1;
       if s.epoch==epoch&&s.epoch==native::EPOCH.load(Ordering::Acquire)&&(util::is_current(s.source)||matches!(purpose,Purpose::Clipboard(false))){match purpose{
        Purpose::Copy=>{},
-       Purpose::Detect=>{cache=Some((s.clone(),Instant::now()));let _=cx.update(|cx|{hide(&mut bee,cx);bee=Some(open_bee(cx,tx.clone(),s));});},
-       Purpose::Verify(old)=>if old.same_selection(&s){let _=tx.try_send(Command::Translate(s,false));},
-       Purpose::Preview|Purpose::Clipboard(false)=>{generation+=1;pool.cancel_pending();mailbox=None;request_snapshot=false;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);panel=Some(open_panel(cx,tx.clone(),s,true));});},
-       Purpose::Clipboard(true)=>{cache=Some((s.clone(),Instant::now()));let _=cx.update(|cx|{hide(&mut bee,cx);bee=Some(open_bee(cx,tx.clone(),s));});}
+       Purpose::Detect=>{cache=Some((s.clone(),Instant::now()));let _=cx.update(|cx|{hide(&mut bee,cx);bee=open_bee(cx,tx.clone(),s);});},
+       Purpose::Verify(old)=>if old.same_selection(&s){let _=tx.try_send(Command::Translate(s,false));}else{let _=cx.update(|cx|hide(&mut bee,cx));},
+       Purpose::Preview|Purpose::Clipboard(false)=>{generation+=1;pool.cancel_pending();mailbox=None;request_snapshot=false;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);panel=open_panel(cx,tx.clone(),s,true);});},
+       Purpose::Clipboard(true)=>{cache=Some((s.clone(),Instant::now()));let _=cx.update(|cx|{hide(&mut bee,cx);bee=open_bee(cx,tx.clone(),s);});}
       }}
      },
      Outcome::Copied(reply,_)=>{if pending.as_ref().is_some_and(|(i,_)|*i==reply){pending=None;let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="已复制译文".into();cx.notify();});});}},
-     Outcome::Empty(reply,status)=>{let _=status;if pending.as_ref().is_some_and(|(i,_)|*i==reply){pending=None;let _=cx.update(|cx|hide(&mut bee,cx));}},
-     Outcome::Fault(reply,code)=>{let _=code;if pending.as_ref().is_some_and(|(i,_)|*i==reply){pending=None;let _=cx.update(|cx|hide(&mut bee,cx));}},
+     Outcome::Empty(reply,status)=>{if pending.as_ref().is_some_and(|(i,_)|*i==reply){let purpose=pending.take().unwrap().1;let copy=matches!(purpose,Purpose::Copy);let manual=matches!(purpose,Purpose::Preview|Purpose::Clipboard(false));let _=cx.update(|cx|{hide(&mut bee,cx);if copy{if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，剪贴板可能已变化或被占用，请重试".into();cx.notify();});}}if manual{let message=selection_status(status).to_owned();if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});}else{form=open_form(cx,tx.clone(),cfg.clone(),message);}}});}},
+     Outcome::Fault(reply,code)=>{let _=code;if pending.as_ref().is_some_and(|(i,_)|*i==reply){let copy=matches!(pending.take().unwrap().1,Purpose::Copy);let _=cx.update(|cx|{hide(&mut bee,cx);if copy{if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，请稍后重试".into();cx.notify();});}}});}},
      Outcome::Paused=>{let _=cx.update(|cx|{hide(&mut bee,cx);if let Some(f)=form{let _=f.update(cx,|s,_,cx|{s.status="选区检测已暂停，请从托盘手动恢复".into();cx.notify();});}});},_=>{}
     }}
     while let Ok(command)=rx.try_recv(){match command{
      Command::Click(s)=>if cache.as_ref().is_some_and(|(c,at)|c.same_selection(&s)&&at.elapsed()<Duration::from_millis(TTL_MS))&&s.epoch==epoch&&util::is_current(s.source){id+=1;coordinator.request(id,s.source,epoch,util::cursor(),s.clipboard_sequence.is_some());pending=Some((id,Purpose::Verify(s)));},
      Command::Translate(s,snapshot)=>{
       if snapshot{let valid=panel.is_some_and(|p|p.update(cx,|p,_,_|p.snapshot&&p.selection.same_selection(&s)).unwrap_or(false));if !valid{continue;}}
       else if s.epoch!=native::EPOCH.load(Ordering::Acquire)||!util::is_current(s.source){continue;}
       generation+=1;pool.cancel_pending();request_snapshot=snapshot;request_epoch=s.epoch;let g=generation;let config=cfg.clone();let text=s.text.clone();
-      let _=cx.update(|cx|{hide(&mut bee,cx);if !snapshot{hide(&mut panel,cx);panel=Some(open_panel(cx,tx.clone(),s,false));}if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=true;p.status="正在翻译…".into();p.body.clear();cx.notify();});}});
+      let _=cx.update(|cx|{hide(&mut bee,cx);if !snapshot{hide(&mut panel,cx);panel=open_panel(cx,tx.clone(),s,false);}if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=true;p.status="正在翻译…".into();p.body.clear();cx.notify();});}});
       let error=if saving{Some("设置正在保存，请稍后重新选择或重试")}else if !config_valid{Some("配置损坏，恢复并保存设置前不能发送翻译请求")}else if work_tx.try_send(Work::Request(g,config,text)).is_err(){Some("设置操作繁忙，请稍后重试")}else{None};
       if let Some(error)=error{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=false;p.status=error.into();cx.notify();});});}
      },
      Command::Save(next,key) if !saving=>{
-      saving=work_tx.try_send(Work::Save(cfg.clone(),next,key)).is_ok();if !saving{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.busy=false;f.status="设置操作繁忙，请稍后重试".into();cx.notify();});});}
+      saving=if edit_active.is_some()||!edits.is_empty(){false}else{work_tx.try_send(Work::Save(cfg.clone(),next,key)).is_ok()};if !saving{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.busy=false;f.status="设置操作繁忙，请稍后重试".into();cx.notify();});});}
      },
-     Command::Copy(text,sequence)=>{if let Some(source)=util::source(unsafe{windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()},epoch){id+=1;coordinator.copy(id,source,epoch,text,sequence);pending=Some((id,Purpose::Copy));}},
-     Command::Edit(field,mut intent)=>{if let ClipboardIntent::Paste{ticket,at,..}=&mut intent{if let Some((target,old,new,completed))=&last_cut{if target==&field&&*ticket==*old&&*at<=*completed{*ticket=*new;}}}if edits.len()<4{let ignore=matches!(&intent,ClipboardIntent::Paste{at,..} if failed_copy.is_some_and(|failed|*at<=failed));if !ignore{edits.push_back((field,intent));}}else{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status="剪贴板操作繁忙，请稍后重试".into();cx.notify();});});}},
+     Command::Copy(text,sequence)=>{if let Some(source)=util::source(unsafe{windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()},epoch){id+=1;coordinator.copy(id,source,epoch,text,sequence);pending=Some((id,Purpose::Copy));}else{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，请返回普通桌面后重试".into();cx.notify();});});}},
+     Command::Edit(field,mut intent)=>{
+      #[cfg(feature="test-fixtures")]super::diagnostics::event("editor_intent",match &intent{ClipboardIntent::Paste{ticket,..}=>serde_json::json!({"kind":"paste","ticket":ticket}),ClipboardIntent::Copy{ticket,cut,..}=>serde_json::json!({"kind":if *cut{"cut"}else{"copy"},"ticket":ticket})});if let ClipboardIntent::Paste{ticket,at,..}=&mut intent{if let Some((target,old,new,completed))=&last_applied_edit{if target==&field&&*ticket==*old&&*at<=*completed{*ticket=*new;}}}if edits.len()<4{let ignore=matches!(&intent,ClipboardIntent::Paste{at,..} if failed_copy.is_some_and(|failed|*at<=failed));if !ignore{edits.push_back((field,intent));}else{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status="先前复制未完成，关联粘贴已取消，请重试".into();cx.notify();});});}}else{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status="剪贴板操作繁忙，请稍后重试".into();cx.notify();});});}},
      Command::Speak(text)=>super::speech::speak(text),Command::Close=>{generation+=1;pool.cancel_pending();mailbox=None;let _=cx.update(|cx|{hide(&mut panel,cx);hide(&mut bee,cx);});},
      Command::Update=>{let queued=super::integration::open_updates();let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=if queued{"正在检查 Windows 稳定版更新…"}else{"更新检查繁忙，请稍后重试"}.into();cx.notify();});});},
      Command::Drag(at)=>{if let Some(p)=panel{let mut origin=None;let _=p.update(cx,|p,w,_|{if !p.snapshot{if let Ok(h)=native::hwnd(w){let mut rect=windows::Win32::Foundation::RECT::default();if unsafe{windows::Win32::UI::WindowsAndMessaging::GetWindowRect(h,&mut rect)}.is_ok(){origin=Some([rect.left,rect.top]);}}}});if let Some(origin)=origin{dragging=Some((at,origin));}}},_=>{}
     }}
     if edit_active.is_none(){if let Some((field,intent))=edits.pop_front(){
      let source=form.and_then(|f|f.update(cx,|_,w,_|native::hwnd(w).ok().and_then(|h|util::source(h,epoch))).ok().flatten());
-     if let Some(source)=source{id+=1;match &intent{ClipboardIntent::Paste{sequence,..}=>coordinator.paste(id,source,epoch,*sequence),ClipboardIntent::Copy{text,sequence,..}=>coordinator.copy(id,source,epoch,text.clone(),*sequence)}edit_active=Some((id,field,intent));}
+     if let Some(source)=source{id+=1;match &intent{ClipboardIntent::Paste{sequence,..}=>coordinator.paste(id,source,epoch,*sequence),ClipboardIntent::Copy{text,sequence,..}=>coordinator.copy(id,source,epoch,text.clone(),*sequence)}edit_active=Some((id,field,intent));}else{edits.clear();last_settings_status=Some("旧设置窗口已关闭，未完成的输入操作已取消".into());}
     }}
     while let Ok(message)=bg_rx.try_recv(){match message{
-     Background::Saved(result,warning)=>{saving=false;let committed=result.is_ok();let status=match result{Ok(saved)=>{cfg=saved;config_valid=true;_desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);warning.unwrap_or_else(||"已保存".into())},Err(e)=>e};let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.busy=false;f.status=status;if committed{f.cfg=cfg.clone();f.clear=false;f.fields[0].update(cx,|key,cx|key.set_value("",cx));}cx.notify();});});},
+     Background::Saved(result,warning)=>{saving=false;let committed=result.is_ok();let status=match result{Ok(saved)=>{cfg=saved;config_valid=true;_desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);warning.unwrap_or_else(||"已保存".into())},Err(e)=>e};last_settings_status=Some(status.clone());let editing=edit_active.is_some()||!edits.is_empty();let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{let signature=f.save_revision.take();let unchanged=signature.as_ref().is_some_and(|s|s==&f.signature(cx))&&!editing;f.busy=false;f.status=status;if committed{if unchanged{f.cfg=cfg.clone();}else{f.status.push_str("；提交后的新修改尚未保存");}if signature.as_ref().is_some_and(|(_,flags)|flags[4]==f.clear){f.clear=false;}if !editing&&signature.as_ref().is_some_and(|(fields,_)|fields[0]==f.fields[0].read(cx).revision()){f.fields[0].update(cx,|key,cx|key.set_value("",cx));}}cx.notify();});});},
      Background::Request(g,result) if g==generation&&(request_snapshot||request_epoch==native::EPOCH.load(Ordering::Acquire))&&panel.is_some_and(|p|p.update(cx,|_,_,_|()).is_ok())=>match result{Ok(request)=>mailbox=Some((g,pool.submit(request))),Err(e)=>{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=false;p.status=e;cx.notify();});});}},_=>{}
     }}
     if let Some(status)=super::integration::update_result(){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=status;cx.notify();});});}
     if !native::HOTKEY_OK.load(Ordering::Acquire){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{if !f.status.contains("快捷键"){f.status="全局快捷键注册失败（冲突或格式无效），请修改后保存".into();cx.notify();}});});}

```

## src/platform/windows/input.rs
```diff
--- round1/src/platform/windows/input.rs
+++ current/src/platform/windows/input.rs
@@ -270,9 +270,9 @@
         self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
     }
 
     fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
-        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
+        super::text_model::utf16_range(&self.content, range_utf16.clone())
     }
 
     fn previous_boundary(&self, offset: usize) -> usize {
         self.content
@@ -692,9 +692,9 @@
         range: Range<usize>,
         window: &mut Window,
         cx: &mut Context<Self>,
     ) -> Option<u64> {
-        if self.revision != ticket || self.selected_range != range {
+        if self.revision != ticket || self.selected_range != range || self.marked_range.is_some() {
             return None;
         }
         let range = self.range_to_utf16(&range);
         self.replace_text_in_range(Some(range), "", window, cx);
@@ -705,12 +705,26 @@
         ticket: u64,
         value: &str,
         w: &mut Window,
         cx: &mut Context<Self>,
-    ) {
-        if self.revision == ticket && self.focus_handle.is_focused(w) {
-            self.replace_text_in_range(None, &value.replace(['\r', '\n'], " "), w, cx);
-        }
+    ) -> Option<u64> {
+        if self.revision != ticket
+            || !self.focus_handle.is_focused(w)
+            || !w.is_window_active()
+            || self.marked_range.is_some()
+        {
+            return None;
+        }
+        let value = value.replace(['\r', '\n'], " ");
+        let size = self.content.len() - self.selected_range.len() + value.len();
+        if size > 32768 {
+            return None;
+        }
+        self.replace_text_in_range(None, &value, w, cx);
+        Some(self.revision)
+    }
+    pub fn revision(&self) -> u64 {
+        self.revision
     }
     pub fn new(cx: &mut Context<Self>, placeholder: &str) -> Self {
         Self {
             masked: false,

```

## src/platform/windows/native.rs
```diff
--- round1/src/platform/windows/native.rs
+++ current/src/platform/windows/native.rs
@@ -32,8 +32,9 @@
     Restart,
     About,
     Quit,
     ClipboardChanged,
+    IpcUnavailable,
 }
 static EVENTS: OnceLock<mpsc::SyncSender<Event>> = OnceLock::new();
 pub static EPOCH: AtomicU64 = AtomicU64::new(1);
 pub static SYSTEM_EPOCH: AtomicU64 = AtomicU64::new(1);
@@ -48,8 +49,9 @@
 static MOUSE_AT: AtomicU64 = AtomicU64::new(0);
 static MOUSE_ROOT: AtomicU64 = AtomicU64::new(0);
 static MOUSE_PID: AtomicU32 = AtomicU32::new(0);
 static MOUSE_POINT: AtomicU64 = AtomicU64::new(0);
+pub static CLIPBOARD_HELPER_PID: AtomicU32 = AtomicU32::new(0);
 static CLIPBOARD_SEQUENCE: AtomicU32 = AtomicU32::new(0);
 static CLIPBOARD_ENABLED: AtomicBool = AtomicBool::new(false);
 pub static HOTKEY_OK: AtomicBool = AtomicBool::new(true);
 static HOTKEY: OnceLock<Mutex<(bool, String)>> = OnceLock::new();
@@ -403,9 +405,15 @@
                 if CLIPBOARD_ENABLED.load(Ordering::Acquire) && {
                     let sequence = GetClipboardSequenceNumber();
                     sequence != 0 && CLIPBOARD_SEQUENCE.swap(sequence, Ordering::AcqRel) != sequence
                 } {
-                    send(Event::ClipboardChanged);
+                    let mut pid = 0;
+                    if let Ok(owner) = GetClipboardOwner() {
+                        GetWindowThreadProcessId(owner, Some(&mut pid));
+                    }
+                    if pid == 0 || pid != CLIPBOARD_HELPER_PID.load(Ordering::Acquire) {
+                        send(Event::ClipboardChanged);
+                    }
                 }
             }
             WM_TIMER if w.0 == 2 => {
                 if !SESSION_NOTIFICATIONS.load(Ordering::Acquire) {

```
