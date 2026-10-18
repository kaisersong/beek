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

R2 new m1: the PID atomic and Child::drop live in the PARENT, not in the disposable helper. Abnormal helper death still causes parent slot reaping and parent Child::drop CAS cleanup. A crashed parent loses its atomic address space entirely. If actual helper exit has not been confirmed, the OS cannot reuse its PID; after exit the parent reaps on the 10ms loop. This is not permanent stale filtering. The formerly omitted parent implementation is below; please verify rather than adding process/file calls to the native callback.
```rust
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

```

Other R2 minor issues are fixed: editing-blocked Save explicitly says to wait for clipboard edits; unavailable form feedback uses an accurate unavailable wording and updates a live form; manual Preview/explicit Clipboard Fault receives the same visible/reopenable feedback as Empty. Normal Settings/snapshot windows also clamp to the work area before activation. Relevant current code:
```rust
     Outcome::Fault(reply,code)=>{let _=code;if pending.as_ref().is_some_and(|(i,_)|*i==reply){let purpose=pending.take().unwrap().1;let copy=matches!(purpose,Purpose::Copy);let manual=matches!(purpose,Purpose::Preview|Purpose::Clipboard(false));if manual{last_settings_status=Some("选区或剪贴板检测超时、繁忙或已取消，请重试".into());}let _=cx.update(|cx|{hide(&mut bee,cx);if copy{if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，请稍后重试".into();cx.notify();});}}if manual{let message=last_settings_status.clone().unwrap();if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});}else{form=open_form(cx,tx.clone(),cfg.clone(),message);}}});}},
     Outcome::Paused=>{let _=cx.update(|cx|{hide(&mut bee,cx);if let Some(f)=form{let _=f.update(cx,|s,_,cx|{s.status="选区检测已暂停，请从托盘手动恢复".into();cx.notify();});}});},_=>{}
    }}
    while let Ok(command)=rx.try_recv(){match command{
     Command::Click(s)=>if cache.as_ref().is_some_and(|(c,at)|c.same_selection(&s)&&at.elapsed()<Duration::from_millis(TTL_MS))&&s.epoch==epoch&&util::is_current(s.source){id+=1;coordinator.request(id,s.source,epoch,util::cursor(),s.clipboard_sequence.is_some());pending=Some((id,Purpose::Verify(s)));},
     Command::Translate(s,snapshot)=>{
      if snapshot{let valid=panel.is_some_and(|p|p.update(cx,|p,_,_|p.snapshot&&p.selection.same_selection(&s)).unwrap_or(false));if !valid{continue;}}
      else if s.epoch!=native::EPOCH.load(Ordering::Acquire)||!util::is_current(s.source){continue;}
      generation+=1;pool.cancel_pending();request_snapshot=snapshot;request_epoch=s.epoch;let g=generation;let config=cfg.clone();let text=s.text.clone();
      let _=cx.update(|cx|{hide(&mut bee,cx);if !snapshot{hide(&mut panel,cx);panel=open_panel(cx,tx.clone(),s,false);}if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=true;p.status="正在翻译…".into();p.body.clear();cx.notify();});}});
      let error=if saving{Some("设置正在保存，请稍后重新选择或重试")}else if !config_valid{Some("配置损坏，恢复并保存设置前不能发送翻译请求")}else if work_tx.try_send(Work::Request(g,config,text)).is_err(){Some("设置操作繁忙，请稍后重试")}else{None};
      if let Some(error)=error{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=false;p.status=error.into();cx.notify();});});}
     },
     Command::Save(next,key) if !saving=>{
      saving=if edit_active.is_some()||!edits.is_empty(){false}else{work_tx.try_send(Work::Save(cfg.clone(),next,key)).is_ok()};if !saving{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.busy=false;f.status=if edit_active.is_some()||!edits.is_empty(){"请等待剪贴板编辑完成后再保存"}else{"设置操作繁忙，请稍后重试"}.into();cx.notify();});});}
     },
     Command::Copy(text,sequence)=>{if let Some(source)=util::source(unsafe{windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()},epoch){id+=1;coordinator.copy(id,source,epoch,text,sequence);pending=Some((id,Purpose::Copy));}else{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，请返回普通桌面后重试".into();cx.notify();});});}},
     Command::Edit(field,mut intent)=>{
      #[cfg(feature="test-fixtures")]super::diagnostics::event("editor_intent",match &intent{ClipboardIntent::Paste{ticket,..}=>serde_json::json!({"kind":"paste","ticket":ticket}),ClipboardIntent::Copy{ticket,cut,..}=>serde_json::json!({"kind":if *cut{"cut"}else{"copy"},"ticket":ticket})});if let ClipboardIntent::Paste{ticket,at,..}=&mut intent{if let Some((target,old,new,completed))=&last_applied_edit{if target==&field&&*ticket==*old&&*at<=*completed{*ticket=*new;}}}if edits.len()<4{let ignore=matches!(&intent,ClipboardIntent::Paste{at,..} if failed_copy.is_some_and(|failed|*at<=failed));if !ignore{edits.push_back((field,intent));}else{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status="先前复制未完成，关联粘贴已取消，请重试".into();cx.notify();});});}}else{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status="剪贴板操作繁忙，请稍后重试".into();cx.notify();});});}},
     Command::Speak(text)=>super::speech::speak(text),Command::Close=>{generation+=1;pool.cancel_pending();mailbox=None;let _=cx.update(|cx|{hide(&mut panel,cx);hide(&mut bee,cx);});},
     Command::Update=>{let queued=super::integration::open_updates();let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=if queued{"正在检查 Windows 稳定版更新…"}else{"更新检查繁忙，请稍后重试"}.into();cx.notify();});});},
     Command::Drag(at)=>{if let Some(p)=panel{let mut origin=None;let _=p.update(cx,|p,w,_|{if !p.snapshot{if let Ok(h)=native::hwnd(w){let mut rect=windows::Win32::Foundation::RECT::default();if unsafe{windows::Win32::UI::WindowsAndMessaging::GetWindowRect(h,&mut rect)}.is_ok(){origin=Some([rect.left,rect.top]);}}}});if let Some(origin)=origin{dragging=Some((at,origin));}}},_=>{}
    }}
    if edit_active.is_none(){if let Some((field,intent))=edits.pop_front(){
     let source=form.and_then(|f|f.update(cx,|_,w,_|native::hwnd(w).ok().and_then(|h|util::source(h,epoch))).ok().flatten());
     if let Some(source)=source{id+=1;match &intent{ClipboardIntent::Paste{sequence,..}=>coordinator.paste(id,source,epoch,*sequence),ClipboardIntent::Copy{text,sequence,..}=>coordinator.copy(id,source,epoch,text.clone(),*sequence)}edit_active=Some((id,field,intent));}else{edits.clear();let message="设置窗口已不可用，未完成的输入操作已取消".to_owned();last_settings_status=Some(message.clone());let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});});}
    }}
    while let Ok(message)=bg_rx.try_recv(){match message{
     Background::Saved(result,warning)=>{saving=false;let committed=result.is_ok();let status=match result{Ok(saved)=>{cfg=saved;config_valid=true;_desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);warning.unwrap_or_else(||"已保存".into())},Err(e)=>e};last_settings_status=Some(status.clone());let editing=edit_active.is_some()||!edits.is_empty();let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{let signature=f.save_revision.take();let unchanged=signature.as_ref().is_some_and(|s|s==&f.signature(cx))&&!editing;f.busy=false;f.status=status;if committed{if unchanged{f.cfg=cfg.clone();}else{f.status.push_str("；提交后的新修改尚未保存");}if signature.as_ref().is_some_and(|(_,flags)|flags[4]==f.clear){f.clear=false;}if !editing&&signature.as_ref().is_some_and(|(fields,_)|fields[0]==f.fields[0].read(cx).revision()){f.fields[0].update(cx,|key,cx|key.set_value("",cx));}}cx.notify();});});},
     Background::Request(g,result) if g==generation&&(request_snapshot||request_epoch==native::EPOCH.load(Ordering::Acquire))&&panel.is_some_and(|p|p.update(cx,|_,_,_|()).is_ok())=>match result{Ok(request)=>mailbox=Some((g,pool.submit(request))),Err(e)=>{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=false;p.status=e;cx.notify();});});}},_=>{}
    }}
    if let Some(status)=super::integration::update_result(){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=status;cx.notify();});});}
    if !native::HOTKEY_OK.load(Ordering::Acquire){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{if !f.status.contains("快捷键"){f.status="全局快捷键注册失败（冲突或格式无效），请修改后保存".into();cx.notify();}});});}

```
