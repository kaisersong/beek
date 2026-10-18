# Windows implementation review snapshot

Only this subsystem is in scope. Full G5 is NOT passed. G0-D allows internal development. Do not treat missing external signing/multi-monitor/soak evidence as a code bug, and do not approve public release. Review adversarially for concrete reproducible bugs, not generic concerns. Read permission includes continuing this same attachment if truncated.

## src/platform/windows/ui.rs
```
use super::{
    input::{self, ClipboardIntent, TextInput},
    native::{self, Event},
    selection::{Coordinator, Outcome},
    store::Store,
    util,
};
use crate::{
    core::{
        policy::TTL_MS,
        protocol::{Rect, Selection, Source},
        settings::{KeyEdit, Settings},
        stream::{self, Mailbox, Pool, Progress},
    },
    icons::IconKind,
};
use gpui::{prelude::*, *};
use std::{
    collections::VecDeque,
    sync::{atomic::Ordering, mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

enum Command {
    Click(Selection),
    Translate(Selection, bool),
    Copy(String, u32),
    Edit(Entity<TextInput>, ClipboardIntent),
    Speak(String),
    Close,
    Save(Settings, KeyEdit),
    Update,
    Drag([i32; 2]),
}
type Tx = mpsc::SyncSender<Command>;
actions!(
    windows_ui,
    [
        NextField,
        PreviousField,
        SaveFields,
        TranslateSnapshot,
        CloseView,
        CopyPreview
    ]
);
struct Host;
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
struct Bee {
    selection: Selection,
    tx: Tx,
    loading: bool,
}
impl Render for Bee {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let image = if self.loading {
            IconKind::Disabled
        } else {
            IconKind::Default
        }
        .render_image(cx);
        div()
            .id("selection-bee")
            .size_full()
            .cursor_pointer()
            .child(img(image).size_full())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if !this.loading && event.click_count == 1 {
                        this.loading = true;
                        let _ = this.tx.try_send(Command::Click(this.selection.clone()));
                        cx.notify();
                    }
                }),
            )
    }
}
fn button(
    label: &'static str,
    id: &'static str,
    command: impl Fn(&mut Panel) + 'static,
    cx: &mut Context<Panel>,
) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .bg(rgb(0xffd359))
        .text_color(rgb(0x282828))
        .cursor_pointer()
        .child(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if event.click_count > 1 {
                    return;
                }
                command(this);
                cx.notify();
            }),
        )
}
struct Panel {
    active: bool,
    focus: FocusHandle,
    selection: Selection,
    body: String,
    status: String,
    snapshot: bool,
    tx: Tx,
}
impl Panel {
    fn translate(&mut self, _: &TranslateSnapshot, _: &mut Window, cx: &mut Context<Self>) {
        if !self.active {
            self.active = true;
            let _ = self
                .tx
                .try_send(Command::Translate(self.selection.clone(), true));
        }
        cx.notify();
    }
    fn copy(&mut self, _: &CopyPreview, _: &mut Window, _: &mut Context<Self>) {
        if !self.body.is_empty() {
            let _ = self.tx.try_send(Command::Copy(self.body.clone(), unsafe {
                windows::Win32::System::DataExchange::GetClipboardSequenceNumber()
            }));
        }
    }
    fn close(&mut self, _: &CloseView, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }
}
impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (pron, body) = stream::split_pron(&self.body);
        #[cfg(feature = "test-fixtures")]
        super::diagnostics::event(
            "panel_layout",
            serde_json::json!({"width":f32::from(window.viewport_size().width),"height":f32::from(window.viewport_size().height)}),
        );
        div()
            .id("translation-card")
            .w_full()
            .h(window.viewport_size().height)
            .p_4()
            .pb(px(64.))
            .relative()
            .rounded_xl()
            .bg(rgb(0xfffdf6))
            .text_color(rgb(0x292929))
            .border_1()
            .border_color(rgb(0xe8dfc9))
            .flex()
            .flex_col()
            .gap_3()
            .key_context("BeeKPreview")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::translate))
            .on_action(cx.listener(Self::close))
            .on_action(cx.listener(Self::copy))
            .child(
                div()
                    .id("drag-header")
                    .flex()
                    .justify_between()
                    .cursor(gpui::CursorStyle::ClosedHand)
                    .child(if self.snapshot {
                        "BeeK · 本地选区快照"
                    } else {
                        "BeeK · 翻译"
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| {
                            let _ = this.tx.try_send(Command::Drag(util::cursor()));
                        }),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x78716a))
                    .child(self.status.clone()),
            )
            .child(
                div()
                    .text_sm()
                    .max_h(px(80.))
                    .overflow_hidden()
                    .child(self.selection.text.clone()),
            )
            .when_some(pron, |d, p| d.child(div().text_sm().child(p)))
            .child(
                div()
                    .id("translation-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .text_sm()
                    .child(body),
            )
            .child(
                div()
                    .flex()
                    .absolute()
                    .bottom(px(16.))
                    .left(px(16.))
                    .right(px(16.))
                    .flex_shrink_0()
                    .h(px(32.))
                    .gap_2()
                    .when(self.snapshot, |d| {
                        d.child(button(
                            "翻译 (Enter)",
                            "translate",
                            |s| {
                                if !s.active {
                                    s.active = true;
                                    let _ = s
                                        .tx
                                        .try_send(Command::Translate(s.selection.clone(), true));
                                }
                            },
                            cx,
                        ))
                    })
                    .child(button(
                        "复制译文",
                        "copy",
                        |s| {
                            if !s.body.is_empty() {
                                let _ = s.tx.try_send(Command::Copy(s.body.clone(),unsafe{windows::Win32::System::DataExchange::GetClipboardSequenceNumber()}));
                            }
                        },
                        cx,
                    ))
                    .child(button(
                        "朗读原文",
                        "speak",
                        |s| {
                            let _ = s.tx.try_send(Command::Speak(s.selection.text.clone()));
                        },
                        cx,
                    ))
                    .child(button(
                        "关闭",
                        "close",
                        |s| {
                            let _ = s.tx.try_send(Command::Close);
                        },
                        cx,
                    )),
            )
    }
}
struct Form {
    _subscriptions: Vec<Subscription>,
    fields: Vec<Entity<TextInput>>,
    cfg: Settings,
    status: String,
    busy: bool,
    clear: bool,
    tx: Tx,
}
impl Form {
    fn next(&mut self, _: &NextField, w: &mut Window, cx: &mut Context<Self>) {
        let n = self
            .fields
            .iter()
            .position(|f| f.focus_handle(cx).is_focused(w))
            .unwrap_or(0);
        self.fields[(n + 1) % self.fields.len()].read(cx).focus(w);
    }
    fn previous(&mut self, _: &PreviousField, w: &mut Window, cx: &mut Context<Self>) {
        let n = self
            .fields
            .iter()
            .position(|f| f.focus_handle(cx).is_focused(w))
            .unwrap_or(0);
        self.fields[(n + self.fields.len() - 1) % self.fields.len()]
            .read(cx)
            .focus(w);
    }
    fn save(&mut self, _: &SaveFields, _: &mut Window, cx: &mut Context<Self>) {
        self.submit(cx);
    }
    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let mut cfg = self.cfg.clone();
        cfg.provider.model = self.fields[1].read(cx).value().trim().into();
        cfg.provider.base_url = self.fields[2].read(cx).value().trim().into();
        cfg.hotkey = self.fields[3].read(cx).value().trim().into();
        if cfg.hotkey_enabled && native::parse_hotkey(&cfg.hotkey).is_none() {
            self.status = "快捷键格式例如 Ctrl+Alt+T；需修饰键和一个英文字母".into();
            cx.notify();
            return;
        }
        if let Err(e) = cfg.validate() {
            self.status = e;
            cx.notify();
            return;
        }
        let key = self.fields[0].read(cx).value();
        let edit = if self.clear {
            KeyEdit::Clear
        } else if key.trim().is_empty() {
            KeyEdit::Keep
        } else {
            KeyEdit::Replace(key)
        };
        if self.tx.try_send(Command::Save(cfg, edit)).is_ok() {
            self.busy = true;
            self.status = "正在保存…".into();
            cx.notify();
        }
    }
}
impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let labels = [
            "API Key（留空保留原值；输入后显示为星号）",
            "模型 / 推理接入点",
            "完整 API 地址（chat/completions）",
            "全局快捷键（默认 Ctrl+Alt+T）",
        ];
        let mut view = div()
            .id("settings")
            .size_full()
            .p_6()
            .bg(rgb(0xf8f7f3))
            .text_color(rgb(0x252525))
            .flex()
            .flex_col()
            .gap_3()
            .overflow_y_scroll()
            .key_context("BeeKSettings")
            .on_action(cx.listener(Self::next))
            .on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::save))
            .child(div().text_xl().child("BeeK 设置"));
        for (i, label) in labels.into_iter().enumerate() {
            view = view
                .child(div().text_sm().child(label))
                .child(self.fields[i].clone());
        }
        let flags = [
            ("选中文本后显示蜜蜂按钮", self.cfg.selection_enabled),
            ("复制文本后显示（默认关闭）", self.cfg.copy_to_popup),
            ("启用全局快捷键", self.cfg.hotkey_enabled),
            ("登录 Windows 时启动", self.cfg.autostart),
            ("清除已保存的 API Key", self.clear),
        ];
        for (i, (label, on)) in flags.into_iter().enumerate() {
            view = view.child(
                div()
                    .id(i)
                    .cursor_pointer()
                    .text_sm()
                    .child(format!("{} {label}", if on { "☑" } else { "☐" }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |s, _, _, cx| {
                            match i {
                                0 => s.cfg.selection_enabled = !s.cfg.selection_enabled,
                                1 => s.cfg.copy_to_popup = !s.cfg.copy_to_popup,
                                2 => s.cfg.hotkey_enabled = !s.cfg.hotkey_enabled,
                                3 => s.cfg.autostart = !s.cfg.autostart,
                                _ => s.clear = !s.clear,
                            }
                            cx.notify();
                        }),
                    ),
            );
        }
        view.child(
            div()
                .text_sm()
                .text_color(rgb(0x795a22))
                .child(self.status.clone()),
        )
        .when(!native::HOTKEY_OK.load(Ordering::Acquire), |d| {
            d.child("快捷键注册失败，可能被其他应用占用；请修改后保存。")
        })
        .child(
            div()
                .id("save")
                .p_2()
                .rounded_md()
                .bg(rgb(0xffd359))
                .cursor_pointer()
                .child(if self.busy {
                    "保存中…"
                } else {
                    "保存 (Ctrl+S)"
                })
                .on_mouse_down(MouseButton::Left, cx.listener(|s, _, _, cx| s.submit(cx))),
        )
        .child(
            div()
                .id("updates")
                .text_sm()
                .cursor_pointer()
                .child(format!("BeeK {} · 手动检查更新", env!("CARGO_PKG_VERSION")))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, _| {
                        let _ = s.tx.try_send(Command::Update);
                    }),
                ),
        )
        .child(
            div()
                .text_xs()
                .child("仅点击翻译后发送所选文本。剪贴板回退不会模拟 Ctrl+C；不自动下载安装更新。"),
        )
    }
}
fn open_form(cx: &mut App, tx: Tx, cfg: Settings, status: String) -> WindowHandle<Form> {
    let handle = cx
        .open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("BeeK Settings".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(720.), px(780.)),
                    cx,
                ))),
                ..Default::default()
            },
            |w, cx| {
                let mut fields = Vec::new();
                for (i, (label, value)) in [
                    ("API Key", ""),
                    ("model", cfg.provider.model.as_str()),
                    ("API URL", cfg.provider.base_url.as_str()),
                    ("hotkey", cfg.hotkey.as_str()),
                ]
                .into_iter()
                .enumerate()
                {
                    let field = cx.new(|cx| {
                        let mut f = TextInput::new(cx, label);
                        f.set_value(value, cx);
                        if i == 0 {
                            f.mask();
                        }
                        f
                    });
                    fields.push(field);
                }
                fields[0].read(cx).focus(w);
                cx.new(|cx| {
                    let subscriptions = fields
                        .iter()
                        .map(|field| {
                            cx.subscribe(
                                field,
                                |this: &mut Form, field, event: &ClipboardIntent, _| {
                                    let _ = this.tx.try_send(Command::Edit(field, event.clone()));
                                },
                            )
                        })
                        .collect();
                    Form {
                        _subscriptions: subscriptions,
                        fields,
                        cfg,
                        status,
                        busy: false,
                        clear: false,
                        tx,
                    }
                })
            },
        )
        .expect("settings window");
    let _ = handle.update(cx, |_, w, _| native::activate(w));
    handle
}
fn open_bee(cx: &mut App, tx: Tx, s: Selection) -> WindowHandle<Bee> {
    let anchor = s.rects.last().copied().unwrap_or(Rect {
        x: util::cursor()[0] as f64,
        y: util::cursor()[1] as f64,
        width: 1.,
        height: 1.,
    });
    let w = cx
        .open_window(
            WindowOptions {
                titlebar: None,
                show: false,
                focus: false,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(40.), px(40.)),
                ))),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| Bee {
                    selection: s,
                    tx,
                    loading: false,
                })
            },
        )
        .expect("selection window");
    let _ = w.update(cx, |_, window, _| {
        native::show_popup(window, anchor, [40., 40.])
    });
    w
}
fn open_panel(cx: &mut App, tx: Tx, s: Selection, snapshot: bool) -> WindowHandle<Panel> {
    let anchor = s.rects.last().copied().unwrap_or(Rect {
        x: util::cursor()[0] as f64,
        y: util::cursor()[1] as f64,
        width: 1.,
        height: 1.,
    });
    let w = cx
        .open_window(
            WindowOptions {
                titlebar: if snapshot {
                    Some(TitlebarOptions {
                        title: Some("BeeK selection snapshot".into()),
                        ..Default::default()
                    })
                } else {
                    None
                },
                show: snapshot,
                focus: snapshot,
                kind: if snapshot {
                    WindowKind::Normal
                } else {
                    WindowKind::PopUp
                },
                is_movable: snapshot,
                is_resizable: snapshot,
                window_background: WindowBackgroundAppearance::Transparent,
                window_bounds: Some(WindowBounds::Windowed(if snapshot {
                    Bounds::centered(None, size(px(520.), px(420.)), cx)
                } else {
                    Bounds::new(point(px(0.), px(0.)), size(px(520.), px(420.)))
                })),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    if snapshot {
                        window.focus(&focus);
                    }
                    Panel {
                        active: false,
                        focus,
                        status: if s.clipboard_sequence.is_some() {
                            "来自剪贴板 · 仅本地预览，点击翻译后发送".into()
                        } else if snapshot {
                            "已捕获的选区快照 · 点击翻译后发送".into()
                        } else {
                            "正在翻译…".into()
                        },
                        selection: s,
                        body: String::new(),
                        snapshot,
                        tx,
                    }
                })
            },
        )
        .expect("translation window");
    if snapshot {
        let _ = w.update(cx, |_, window, _| native::activate(window));
    } else {
        let _ = w.update(cx, |_, window, _| {
            native::show_popup(window, anchor, [520., 420.])
        });
    }
    w
}
enum Purpose {
    Detect,
    Verify(Selection),
    Preview,
    Clipboard(bool),
    Copy,
}
enum Background {
    Saved(Result<Settings, String>, Option<String>),
    Request(u64, Result<stream::Request, String>),
}
enum Work {
    Save(Settings, Settings, KeyEdit),
    Request(u64, Settings, String),
}
fn hide<T: 'static>(handle: &mut Option<WindowHandle<T>>, cx: &mut App) {
    if let Some(w) = handle.take() {
        let _ = w.update(cx, |_, w, _| w.remove_window());
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
        let loaded = store.load().and_then(|cfg| {
            if cfg.provider.api_key.is_empty() {
                Ok(cfg)
            } else {
                let key = cfg.provider.api_key.clone();
                store.save(&cfg, cfg.clone(), KeyEdit::Replace(key))
            }
        });
        if loaded.is_ok() {
            let _ = store.cleanup();
        }
        loaded
    };
    let valid = loaded.is_ok();
    let initial_error = loaded.as_ref().err().cloned().unwrap_or_default();
    let initial = loaded.unwrap_or_default();
    let pool = Pool::new();
    let (tx, rx) = mpsc::sync_channel(32);
    let (bg_tx, bg_rx) = mpsc::sync_channel(2);
    let (work_tx, work_rx) = mpsc::sync_channel(1);
    let mut cfg = initial;
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
  let host=cx.open_window(WindowOptions{titlebar:None,show:false,focus:false,kind:WindowKind::PopUp,window_bounds:Some(WindowBounds::Windowed(Bounds::new(point(px(0.),px(0.)),size(px(1.),px(1.))))),..Default::default()},|_,cx|cx.new(|_|Host)).expect("lifetime host");
  // A restart child reaches GPUI/GPU window creation before ChildReady. Until
  // ParentExitCommitted and actual parent death it installs no tray or hooks.
  let initialized=(||->Result<_,String>{
   let instance=if restarting{super::lifecycle::child_preflight()?;super::lifecycle::Instance::begin(etx.clone())?.ok_or("重启后会话锁仍被占用")?}else{instance.unwrap()};
   let coordinator=Coordinator::start()?;let desktop=native::Desktop::start(etx)?;desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);Ok((instance,coordinator,desktop))
  })();
  let(mut instance,coordinator,desktop)=match initialized{Ok(v)=>v,Err(e)=>{util::fatal(&e);cx.quit();return;}};
  let mut form=if std::env::args().any(|a|a=="--autostart"){None}else{Some(open_form(cx,tx.clone(),cfg.clone(),initial_error.clone()))};let mut bee:Option<WindowHandle<Bee>>=None;let mut panel:Option<WindowHandle<Panel>>=None;
  cx.spawn(async move|cx|{
   let(_host,_desktop)=(host,desktop);let mut config_valid=valid;let mut epoch=native::EPOCH.load(Ordering::Acquire);let mut trigger=0;let mut id=0;let mut generation=0;let mut pending:Option<(u64,Purpose)>=None;let mut cache:Option<(Selection,Instant)>=None;let mut last_source:Option<Source>=None;let mut mailbox:Option<(u64,Mailbox)>=None;let mut dragging:Option<([i32;2],[i32;2])>=None;let mut saving=false;let mut request_snapshot=false;let mut request_epoch=0;let mut hotkey_requested=false;let mut copy_requested=false;let mut quiet_at=Instant::now();let mut system_epoch=native::SYSTEM_EPOCH.load(Ordering::Acquire);let mut edits:VecDeque<(Entity<TextInput>,ClipboardIntent)>=VecDeque::new();let mut edit_active:Option<(u64,Entity<TextInput>,ClipboardIntent)>=None;let mut failed_copy:Option<Instant>=None;let mut last_cut:Option<(Entity<TextInput>,u64,u64,Instant)>=None;
   loop {
    let context=native::SYSTEM_EPOCH.load(Ordering::Acquire);if context!=system_epoch{system_epoch=context;edits.clear();edit_active=None;failed_copy=None;last_cut=None;pending=None;cache=None;generation+=1;pool.cancel_pending();mailbox=None;request_snapshot=false;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);if let Some(f)=form{let _=f.update(cx,|_,w,_|native::keep_accessible(w));}});}
    let changed=native::EPOCH.load(Ordering::Acquire);
    if let Some(source)=util::current(changed){last_source=Some(source);}
    if changed!=epoch{epoch=changed;quiet_at=Instant::now();pending=None;cache=None;if !request_snapshot{generation+=1;pool.cancel_pending();mailbox=None;}let _=cx.update(|cx|{hide(&mut bee,cx);if panel.is_some_and(|w|w.update(cx,|p,_,_|!p.snapshot).unwrap_or(false)){hide(&mut panel,cx);}});}
    if cache.as_ref().is_some_and(|(s,at)|at.elapsed()>Duration::from_millis(TTL_MS)||!util::is_current(s.source)){cache=None;let _=cx.update(|cx|hide(&mut bee,cx));}
    let t=native::triggered();if edit_active.is_none()&&edits.is_empty()&&cfg.selection_enabled&&!hotkey_requested&&!copy_requested&&pending.as_ref().is_none_or(|(_,p)|matches!(p,Purpose::Detect))&&t!=trigger&&native::quiet_since(t){trigger=t;if let Some(source)=util::current(epoch){id+=1;coordinator.request(id,source,epoch,util::cursor(),false);pending=Some((id,Purpose::Detect));}}
    while let Ok(event)=erx.try_recv(){match event{
     Event::Settings|Event::About=>{let about=matches!(event,Event::About);let info=format!("BeeK {} · Windows x64 · helper {} · {}",env!("CARGO_PKG_VERSION"),&env!("BEEK_BUILD_ID")[..12],coordinator.snapshot());let _=cx.update(|cx|{hide(&mut bee,cx);let active=form.and_then(|f|f.update(cx,|_,w,_|native::activate(w)).ok());if active.is_none(){form=Some(open_form(cx,tx.clone(),cfg.clone(),if about{info.clone()}else{String::new()}));}if about{if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=info;cx.notify();});}}});},
     Event::Restore=>coordinator.restore(),Event::Quit=>{super::lifecycle::exit_deadline();let _=cx.update(|cx|cx.quit());return;},
     Event::Restart=>{if let Err(e)=instance.restart(){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|s,_,cx|{s.status=e;cx.notify();});});}},
     Event::Hotkey=>{hotkey_requested=true;quiet_at=Instant::now();},
     Event::ClipboardChanged=>{copy_requested=true;quiet_at=Instant::now();},
     Event::Clipboard=>{
      if let Some(source)=util::current(epoch).or(last_source).or_else(||util::source(unsafe{windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()},epoch)){id+=1;let auto=matches!(event,Event::ClipboardChanged);if auto{coordinator.request(id,source,epoch,util::cursor(),true);}else{coordinator.explicit_clipboard(id,source,epoch);}pending=Some((id,Purpose::Clipboard(auto)));}
     }
    }}
    if hotkey_requested&&quiet_at.elapsed()>=Duration::from_millis(150)&&native::parse_hotkey(&cfg.hotkey).is_none_or(|(_,key)|unsafe{windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(key as i32)}>=0){hotkey_requested=false;trigger=native::triggered();if let Some(source)=util::current(epoch){id+=1;coordinator.request(id,source,epoch,util::cursor(),false);pending=Some((id,Purpose::Preview));}}
    if copy_requested&&quiet_at.elapsed()>=Duration::from_millis(150){copy_requested=false;if let Some(source)=util::current(epoch){id+=1;coordinator.request(id,source,epoch,util::cursor(),true);pending=Some((id,Purpose::Clipboard(true)));}}
    let restart=instance.restart_events.as_ref().and_then(|rx|rx.try_recv().ok());
    if let Some(event)=restart{match event{
     super::lifecycle::RestartEvent::Ready=>instance.grant_restart(),
     super::lifecycle::RestartEvent::Committed=>{super::lifecycle::exit_deadline();let _=cx.update(|cx|cx.quit());return;},
     super::lifecycle::RestartEvent::Failed(e)=>{instance.restart_events=None;let _=cx.update(|cx|{if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=e;cx.notify();});}else{form=Some(open_form(cx,tx.clone(),cfg.clone(),e));}});}
    }}
    let outcomes={coordinator.result.lock().unwrap().drain(..).collect::<Vec<_>>()};
    for outcome in outcomes{
     let reply=match &outcome{Outcome::Selected(id,_)|Outcome::Empty(id,_)|Outcome::Fault(id,_)|Outcome::Copied(id,_)=>Some(*id),_=>None};
     if edit_active.as_ref().is_some_and(|(id,_,_)|Some(*id)==reply){
      let (_,field,intent)=edit_active.take().unwrap();
      let success=matches!((&intent,&outcome),(ClipboardIntent::Copy{..},Outcome::Copied(..))|(ClipboardIntent::Paste{..},Outcome::Selected(..)));
      if let Some(f)=form{let _=f.update(cx,|f,w,cx|{
       match (&intent,&outcome){
        (ClipboardIntent::Paste{ticket,..},Outcome::Selected(_,selection))=>field.update(cx,|field,cx|field.paste_value(*ticket,&selection.text,w,cx)),
        (ClipboardIntent::Copy{ticket,range,cut:true,..},Outcome::Copied(..))=>{if let Some(revision)=field.update(cx,|field,cx|field.cut_complete(*ticket,range.clone(),w,cx)){last_cut=Some((field.clone(),*ticket,revision,Instant::now()));for (target,intent) in &mut edits{if target==&field{if let ClipboardIntent::Paste{ticket:next,..}=intent{if *next==*ticket{*next=revision;}}}}}},_=>{}
       }
       f.status=if success{"剪贴板操作已完成"}else{"剪贴板操作未完成，内容保持不变，请重试"}.into();cx.notify();
      });}
      if !success{if let ClipboardIntent::Copy{..}=intent{failed_copy=Some(Instant::now());edits.retain(|(_,e)|!matches!(e,ClipboardIntent::Paste{..}));}}
      continue;
     }
     match outcome{
     Outcome::Selected(reply,s) if pending.as_ref().is_some_and(|(expected,_)|*expected==reply)=>{
      let purpose=pending.take().unwrap().1;
      if s.epoch==epoch&&s.epoch==native::EPOCH.load(Ordering::Acquire)&&(util::is_current(s.source)||matches!(purpose,Purpose::Clipboard(false))){match purpose{
       Purpose::Copy=>{},
       Purpose::Detect=>{cache=Some((s.clone(),Instant::now()));let _=cx.update(|cx|{hide(&mut bee,cx);bee=Some(open_bee(cx,tx.clone(),s));});},
       Purpose::Verify(old)=>if old.same_selection(&s){let _=tx.try_send(Command::Translate(s,false));},
       Purpose::Preview|Purpose::Clipboard(false)=>{generation+=1;pool.cancel_pending();mailbox=None;request_snapshot=false;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);panel=Some(open_panel(cx,tx.clone(),s,true));});},
       Purpose::Clipboard(true)=>{cache=Some((s.clone(),Instant::now()));let _=cx.update(|cx|{hide(&mut bee,cx);bee=Some(open_bee(cx,tx.clone(),s));});}
      }}
     },
     Outcome::Copied(reply,_)=>{if pending.as_ref().is_some_and(|(i,_)|*i==reply){pending=None;let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="已复制译文".into();cx.notify();});});}},
     Outcome::Empty(reply,status)=>{let _=status;if pending.as_ref().is_some_and(|(i,_)|*i==reply){pending=None;let _=cx.update(|cx|hide(&mut bee,cx));}},
     Outcome::Fault(reply,code)=>{let _=code;if pending.as_ref().is_some_and(|(i,_)|*i==reply){pending=None;let _=cx.update(|cx|hide(&mut bee,cx));}},
     Outcome::Paused=>{let _=cx.update(|cx|{hide(&mut bee,cx);if let Some(f)=form{let _=f.update(cx,|s,_,cx|{s.status="选区检测已暂停，请从托盘手动恢复".into();cx.notify();});}});},_=>{}
    }}
    while let Ok(command)=rx.try_recv(){match command{
     Command::Click(s)=>if cache.as_ref().is_some_and(|(c,at)|c.same_selection(&s)&&at.elapsed()<Duration::from_millis(TTL_MS))&&s.epoch==epoch&&util::is_current(s.source){id+=1;coordinator.request(id,s.source,epoch,util::cursor(),s.clipboard_sequence.is_some());pending=Some((id,Purpose::Verify(s)));},
     Command::Translate(s,snapshot)=>{
      if snapshot{let valid=panel.is_some_and(|p|p.update(cx,|p,_,_|p.snapshot&&p.selection.same_selection(&s)).unwrap_or(false));if !valid{continue;}}
      else if s.epoch!=native::EPOCH.load(Ordering::Acquire)||!util::is_current(s.source){continue;}
      generation+=1;pool.cancel_pending();request_snapshot=snapshot;request_epoch=s.epoch;let g=generation;let config=cfg.clone();let text=s.text.clone();
      let _=cx.update(|cx|{hide(&mut bee,cx);if !snapshot{hide(&mut panel,cx);panel=Some(open_panel(cx,tx.clone(),s,false));}if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=true;p.status="正在翻译…".into();p.body.clear();cx.notify();});}});
      let error=if saving{Some("设置正在保存，请稍后重新选择或重试")}else if !config_valid{Some("配置损坏，恢复并保存设置前不能发送翻译请求")}else if work_tx.try_send(Work::Request(g,config,text)).is_err(){Some("设置操作繁忙，请稍后重试")}else{None};
      if let Some(error)=error{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=false;p.status=error.into();cx.notify();});});}
     },
     Command::Save(next,key) if !saving=>{
      saving=work_tx.try_send(Work::Save(cfg.clone(),next,key)).is_ok();if !saving{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.busy=false;f.status="设置操作繁忙，请稍后重试".into();cx.notify();});});}
     },
     Command::Copy(text,sequence)=>{if let Some(source)=util::source(unsafe{windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()},epoch){id+=1;coordinator.copy(id,source,epoch,text,sequence);pending=Some((id,Purpose::Copy));}},
     Command::Edit(field,mut intent)=>{if let ClipboardIntent::Paste{ticket,at,..}=&mut intent{if let Some((target,old,new,completed))=&last_cut{if target==&field&&*ticket==*old&&*at<=*completed{*ticket=*new;}}}if edits.len()<4{let ignore=matches!(&intent,ClipboardIntent::Paste{at,..} if failed_copy.is_some_and(|failed|*at<=failed));if !ignore{edits.push_back((field,intent));}}else{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status="剪贴板操作繁忙，请稍后重试".into();cx.notify();});});}},
     Command::Speak(text)=>super::speech::speak(text),Command::Close=>{generation+=1;pool.cancel_pending();mailbox=None;let _=cx.update(|cx|{hide(&mut panel,cx);hide(&mut bee,cx);});},
     Command::Update=>{let queued=super::integration::open_updates();let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=if queued{"正在检查 Windows 稳定版更新…"}else{"更新检查繁忙，请稍后重试"}.into();cx.notify();});});},
     Command::Drag(at)=>{if let Some(p)=panel{let mut origin=None;let _=p.update(cx,|p,w,_|{if !p.snapshot{if let Ok(h)=native::hwnd(w){let mut rect=windows::Win32::Foundation::RECT::default();if unsafe{windows::Win32::UI::WindowsAndMessaging::GetWindowRect(h,&mut rect)}.is_ok(){origin=Some([rect.left,rect.top]);}}}});if let Some(origin)=origin{dragging=Some((at,origin));}}},_=>{}
    }}
    if edit_active.is_none(){if let Some((field,intent))=edits.pop_front(){
     let source=form.and_then(|f|f.update(cx,|_,w,_|native::hwnd(w).ok().and_then(|h|util::source(h,epoch))).ok().flatten());
     if let Some(source)=source{id+=1;match &intent{ClipboardIntent::Paste{sequence,..}=>coordinator.paste(id,source,epoch,*sequence),ClipboardIntent::Copy{text,sequence,..}=>coordinator.copy(id,source,epoch,text.clone(),*sequence)}edit_active=Some((id,field,intent));}
    }}
    while let Ok(message)=bg_rx.try_recv(){match message{
     Background::Saved(result,warning)=>{saving=false;let committed=result.is_ok();let status=match result{Ok(saved)=>{cfg=saved;config_valid=true;_desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);warning.unwrap_or_else(||"已保存".into())},Err(e)=>e};let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.busy=false;f.status=status;if committed{f.cfg=cfg.clone();f.clear=false;f.fields[0].update(cx,|key,cx|key.set_value("",cx));}cx.notify();});});},
     Background::Request(g,result) if g==generation&&(request_snapshot||request_epoch==native::EPOCH.load(Ordering::Acquire))&&panel.is_some_and(|p|p.update(cx,|_,_,_|()).is_ok())=>match result{Ok(request)=>mailbox=Some((g,pool.submit(request))),Err(e)=>{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=false;p.status=e;cx.notify();});});}},_=>{}
    }}
    if let Some(status)=super::integration::update_result(){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=status;cx.notify();});});}
    if !native::HOTKEY_OK.load(Ordering::Acquire){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{if !f.status.contains("快捷键"){f.status="全局快捷键注册失败（冲突或格式无效），请修改后保存".into();cx.notify();}});});}
    let mut completed=false;
    if let Some((g,m))=&mailbox{if *g==generation{let progress=std::mem::replace(&mut *m.lock().unwrap(),Progress::Idle);completed=matches!(progress,Progress::Done(_));if !matches!(progress,Progress::Idle){let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{match progress{Progress::Idle=>{},Progress::Loading=>{p.status="正在翻译…".into();},Progress::Queued=>{p.status="等待前面的实际请求结束…".into();},Progress::Text(text)=>{p.status="正在翻译…".into();p.body=text;},Progress::Done(Ok(text))=>{p.active=false;p.body=text;p.status="完成".into();},Progress::Done(Err(e))=>{p.active=false;p.status=e;}}cx.notify();});});}}}
    if completed{mailbox=None;}
    if let Some((start,origin))=dragging{if unsafe{windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(1)}>=0{dragging=None;}else{let point=util::cursor();if let Some(p)=panel{let _=p.update(cx,|_,w,_|native::move_popup(w,origin[0]+point[0]-start[0],origin[1]+point[1]-start[1]));}}}
    cx.background_executor().timer(Duration::from_millis(16)).await;
   }
  }).detach();
 });
    Ok(())
}

```

## src/platform/windows/input.rs
```
// Adapted from GPUI 0.2.2 examples/input.rs (Zed Industries, Apache-2.0).
// See resources/GPUI-INPUT-NOTICE.md and GPUI-INPUT-LICENSE-APACHE.
use std::ops::Range;

use gpui::{
    actions, div, fill, hsla, point, prelude::*, px, relative, rgb, rgba, size, white, App, Bounds,
    Context, CursorStyle, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine, SharedString, Style,
    TextRun, UTF16Selection, UnderlineStyle, Window,
};
use unicode_segmentation::*;
#[derive(Clone)]
pub enum ClipboardIntent {
    Paste {
        ticket: u64,
        sequence: u32,
        at: std::time::Instant,
    },
    Copy {
        ticket: u64,
        sequence: u32,
        range: Range<usize>,
        text: String,
        cut: bool,
        at: std::time::Instant,
    },
}
impl EventEmitter<ClipboardIntent> for TextInput {}

actions!(
    text_input,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
        Quit,
    ]
);

pub struct TextInput {
    masked: bool,
    scroll_x: Pixels,
    revision: u64,
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

impl TextInput {
    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        _window.focus(&self.focus_handle);
        self.is_selecting = true;

        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        // Clipboard providers can block. The UI asks the isolated reader and
        // applies its result only if this field and edit position are unchanged.
        cx.emit(ClipboardIntent::Paste {
            at: std::time::Instant::now(),
            ticket: self.revision,
            sequence: unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() },
        });
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.masked {
            return;
        }
        if !self.selected_range.is_empty() {
            cx.emit(ClipboardIntent::Copy {
                at: std::time::Instant::now(),
                ticket: self.revision,
                sequence: unsafe {
                    windows::Win32::System::DataExchange::GetClipboardSequenceNumber()
                },
                range: self.selected_range.clone(),
                text: self.content[self.selected_range.clone()].to_string(),
                cut: false,
            });
        }
    }
    fn cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if self.masked {
            return;
        }
        if !self.selected_range.is_empty() {
            cx.emit(ClipboardIntent::Copy {
                at: std::time::Instant::now(),
                ticket: self.revision,
                sequence: unsafe {
                    windows::Win32::System::DataExchange::GetClipboardSequenceNumber()
                },
                range: self.selected_range.clone(),
                text: self.content[self.selected_range.clone()].to_string(),
                cut: true,
            });
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.revision = self.revision.wrapping_add(1);
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }

        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        let mut index = line
            .closest_index_for_x(position.x - bounds.left() + self.scroll_x)
            .min(self.content.len());
        while !self.content.is_char_boundary(index) {
            index = index.saturating_sub(1);
        }
        index
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.revision = self.revision.wrapping_add(1);
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        super::text_model::utf16_offset(&self.content, offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        if self.content.len() - (range.end - range.start) + new_text.len() > 32768 {
            return;
        }
        self.revision = self.revision.wrapping_add(1);
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        if self.content.len() - (range.end - range.start) + new_text.len() > 32768 {
            return;
        }
        self.revision = self.revision.wrapping_add(1);
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|r| super::text_model::utf16_range(new_text, r.clone()))
            .map(|r| r.start + range.start..r.end + range.start)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());

        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() - self.scroll_x + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() - self.scroll_x + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        bounds.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;

        if !self.masked && last_layout.text != self.content {
            return None;
        }
        let utf8_index = last_layout.index_for_x(point.x - bounds.left() + self.scroll_x)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct TextElement {
    input: Entity<TextInput>,
}

struct PrepaintState {
    scroll_x: Pixels,
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = if input.masked {
            "*".repeat(input.content.len()).into()
        } else {
            input.content.clone()
        };
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), hsla(0., 0., 0., 0.2))
        } else {
            (content, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        let visible = (bounds.size.width - px(4.)).max(px(1.));
        let mut scroll = input.scroll_x;
        if cursor_pos - scroll > visible {
            scroll = cursor_pos - visible;
        }
        if cursor_pos < scroll {
            scroll = cursor_pos;
        }
        scroll = scroll.max(px(0.)).min((line.width - visible).max(px(0.)));
        self.input.update(cx, |input, _| input.scroll_x = scroll);

        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() - scroll + cursor_pos, bounds.top()),
                        size(px(2.), bounds.bottom() - bounds.top()),
                    ),
                    gpui::blue(),
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() - scroll + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() - scroll + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    rgba(0x3311ff30),
                )),
                None,
            )
        };
        PrepaintState {
            scroll_x: scroll,
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection)
        }
        let line = prepaint.line.take().unwrap();
        line.paint(
            point(bounds.origin.x - prepaint.scroll_x, bounds.origin.y),
            window.line_height(),
            window,
            cx,
        )
        .unwrap();

        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .key_context("TextInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .bg(rgb(0xeeeeee))
            .line_height(px(30.))
            .text_size(px(24.))
            .child(
                div()
                    .h(px(30. + 4. * 2.))
                    .overflow_hidden()
                    .w_full()
                    .p(px(4.))
                    .bg(white())
                    .child(TextElement { input: cx.entity() }),
            )
    }
}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextInput {
    pub fn value(&self) -> String {
        self.content.to_string()
    }
    pub fn set_value(&mut self, value: &str, cx: &mut Context<Self>) {
        self.revision = self.revision.wrapping_add(1);
        self.content = value.to_owned().into();
        self.selected_range = value.len()..value.len();
        self.marked_range = None;
        cx.notify();
    }
    pub fn mask(&mut self) {
        self.masked = true;
    }
    pub fn cut_complete(
        &mut self,
        ticket: u64,
        range: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        if self.revision != ticket || self.selected_range != range {
            return None;
        }
        let range = self.range_to_utf16(&range);
        self.replace_text_in_range(Some(range), "", window, cx);
        Some(self.revision)
    }
    pub fn paste_value(
        &mut self,
        ticket: u64,
        value: &str,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.revision == ticket && self.focus_handle.is_focused(w) {
            self.replace_text_in_range(None, &value.replace(['\r', '\n'], " "), w, cx);
        }
    }
    pub fn new(cx: &mut Context<Self>, placeholder: &str) -> Self {
        Self {
            masked: false,
            scroll_x: px(0.),
            revision: 0,
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.to_owned().into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
        }
    }
    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }
}
pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("TextInput")),
        KeyBinding::new("delete", Delete, Some("TextInput")),
        KeyBinding::new("left", Left, Some("TextInput")),
        KeyBinding::new("right", Right, Some("TextInput")),
        KeyBinding::new("shift-left", SelectLeft, Some("TextInput")),
        KeyBinding::new("shift-right", SelectRight, Some("TextInput")),
        KeyBinding::new("ctrl-a", SelectAll, Some("TextInput")),
        KeyBinding::new("ctrl-v", Paste, Some("TextInput")),
        KeyBinding::new("ctrl-c", Copy, Some("TextInput")),
        KeyBinding::new("ctrl-x", Cut, Some("TextInput")),
        KeyBinding::new("home", Home, Some("TextInput")),
        KeyBinding::new("end", End, Some("TextInput")),
    ]);
}

```

## src/platform/windows/text_model.rs
```
//! UTF-16 IME offsets converted without splitting a UTF-8 scalar.
use std::ops::Range;

pub fn utf16_offset(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units + ch.len_utf16() > offset {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

pub fn utf16_range(text: &str, range: Range<usize>) -> Range<usize> {
    let start = utf16_offset(text, range.start);
    let end = utf16_offset(text, range.end).max(start);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn composition_selection_is_relative_to_inserted_text() {
        let origin = "prefix中文".len();
        let r = utf16_range("你😀好", 1..3);
        let absolute = origin + r.start..origin + r.end;
        let text = "prefix中文你😀好suffix";
        assert_eq!(&text[absolute], "😀");
    }
    #[test]
    fn surrogate_offsets_never_split_utf8() {
        let s = "a😀中";
        assert_eq!(utf16_offset(s, 2), 1);
        assert_eq!(utf16_offset(s, 3), 5);
        assert_eq!(utf16_offset(s, usize::MAX), s.len());
        assert_eq!(&s[utf16_range(s, 3..2)], "");
    }
}

```

## src/platform/windows/native.rs
```
use super::util;
use crate::core::protocol::Rect;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{
            DataExchange::*, LibraryLoader::*, RemoteDesktop::*, Threading::GetCurrentProcessId,
        },
        UI::{
            Accessibility::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*,
            WindowsAndMessaging::*,
        },
    },
};

#[derive(Clone, Copy)]
pub enum Event {
    Settings,
    Clipboard,
    Hotkey,
    Restore,
    Restart,
    About,
    Quit,
    ClipboardChanged,
}
static EVENTS: OnceLock<mpsc::SyncSender<Event>> = OnceLock::new();
pub static EPOCH: AtomicU64 = AtomicU64::new(1);
pub static SYSTEM_EPOCH: AtomicU64 = AtomicU64::new(1);
pub static MAIN_STARTED: AtomicBool = AtomicBool::new(false);
pub static DESKTOP_ACTIVE: AtomicBool = AtomicBool::new(true);
pub static SESSION_NOTIFICATIONS: AtomicBool = AtomicBool::new(false);
static LOCKED: AtomicBool = AtomicBool::new(false);
static TRIGGER: AtomicU64 = AtomicU64::new(0);
static CLOCK: OnceLock<Instant> = OnceLock::new();
static DRAG: AtomicBool = AtomicBool::new(false);
static MOUSE_INPUT: AtomicBool = AtomicBool::new(false);
static MOUSE_AT: AtomicU64 = AtomicU64::new(0);
static MOUSE_ROOT: AtomicU64 = AtomicU64::new(0);
static MOUSE_PID: AtomicU32 = AtomicU32::new(0);
static MOUSE_POINT: AtomicU64 = AtomicU64::new(0);
static CLIPBOARD_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static CLIPBOARD_ENABLED: AtomicBool = AtomicBool::new(false);
pub static HOTKEY_OK: AtomicBool = AtomicBool::new(true);
static HOTKEY: OnceLock<Mutex<(bool, String)>> = OnceLock::new();
const TRAY: u32 = WM_APP + 1;
const CONFIG: u32 = WM_APP + 2;
fn send(e: Event) {
    if let Some(tx) = EVENTS.get() {
        let _ = tx.try_send(e);
    }
}
fn invalidate(trigger: bool) {
    EPOCH.fetch_add(1, Ordering::AcqRel);
    if trigger {
        TRIGGER.store(
            CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1,
            Ordering::Release,
        );
    }
}
pub fn selection_changed() {
    invalidate(true);
}
pub fn triggered() -> u64 {
    TRIGGER.load(Ordering::Acquire)
}
pub fn quiet_since(trigger: u64) -> bool {
    trigger > 0 && CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1 >= trigger + 120
}
pub fn mouse_anchor(source: crate::core::protocol::Source) -> Option<[i32; 2]> {
    let at = MOUSE_AT.load(Ordering::Acquire);
    let now = CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1;
    if !MOUSE_INPUT.load(Ordering::Acquire)
        || now.saturating_sub(at) > 500
        || MOUSE_ROOT.load(Ordering::Acquire) != source.hwnd
        || MOUSE_PID.load(Ordering::Acquire) != source.pid
    {
        return None;
    }
    let point = MOUSE_POINT.load(Ordering::Acquire);
    Some([(point >> 32) as u32 as i32, point as u32 as i32])
}
fn external(h: HWND) -> bool {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(h, Some(&mut pid));
        pid != 0 && pid != GetCurrentProcessId()
    }
}
unsafe extern "system" fn mouse(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if code >= 0 {
            let data = &*(l.0 as *const MSLLHOOKSTRUCT);
            let h = WindowFromPoint(data.pt);
            if external(h) {
                match w.0 as u32 {
                    WM_LBUTTONDOWN => {
                        DRAG.store(true, Ordering::Release);
                        invalidate(false);
                    }
                    WM_LBUTTONUP => {
                        DRAG.store(false, Ordering::Release);
                        let root = GetAncestor(h, GA_ROOT);
                        let mut pid = 0;
                        GetWindowThreadProcessId(root, Some(&mut pid));
                        MOUSE_ROOT.store(root.0 as u64, Ordering::Release);
                        MOUSE_PID.store(pid, Ordering::Release);
                        MOUSE_POINT.store(
                            ((data.pt.x as u32 as u64) << 32) | data.pt.y as u32 as u64,
                            Ordering::Release,
                        );
                        MOUSE_AT.store(
                            CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1,
                            Ordering::Release,
                        );
                        MOUSE_INPUT.store(true, Ordering::Release);
                        invalidate(true);
                    }
                    WM_MOUSEMOVE if DRAG.load(Ordering::Acquire) => invalidate(false),
                    WM_MOUSEWHEEL | WM_MOUSEHWHEEL => invalidate(false),
                    _ => {}
                }
            }
        }
        CallNextHookEx(None, code, w, l)
    }
}
unsafe extern "system" fn keyboard(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if code >= 0 && external(GetForegroundWindow()) {
            let data = &*(l.0 as *const KBDLLHOOKSTRUCT);
            if ![
                0x10, 0x11, 0x12, 0x5b, 0x5c, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5,
            ]
            .contains(&data.vkCode)
            {
                MOUSE_INPUT.store(false, Ordering::Release);
                let system_chord = data.flags.0 & LLKHF_ALTDOWN.0 != 0
                    || GetAsyncKeyState(VK_LWIN.0 as i32) < 0
                    || GetAsyncKeyState(VK_RWIN.0 as i32) < 0;
                if w.0 as u32 == WM_KEYDOWN || w.0 as u32 == WM_SYSKEYDOWN {
                    invalidate(false);
                } else {
                    invalidate(!system_chord);
                }
            }
        }
        CallNextHookEx(None, code, w, l)
    }
}
unsafe extern "system" fn changed(
    _: HWINEVENTHOOK,
    event: u32,
    h: HWND,
    object: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    // No UIA/COM calls, text, networking or synchronous IPC in callbacks.
    if event == EVENT_OBJECT_LOCATIONCHANGE
        && (unsafe { GetAncestor(h, GA_ROOT) } != unsafe { GetForegroundWindow() }
            || !matches!(object, 0 | -8))
    {
        return;
    }
    invalidate(event == EVENT_OBJECT_TEXTSELECTIONCHANGED);
}
pub fn hwnd(window: &gpui::Window) -> Result<HWND> {
    match HasWindowHandle::window_handle(window)
        .map_err(|_| Error::from_hresult(HRESULT(0x80004005u32 as i32)))?
        .as_raw()
    {
        RawWindowHandle::Win32(h) => Ok(HWND(h.hwnd.get() as *mut _)),
        _ => Err(Error::from_hresult(HRESULT(0x80004005u32 as i32))),
    }
}
unsafe extern "system" fn no_activate(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        if m == WM_MOUSEACTIVATE {
            return LRESULT(MA_NOACTIVATE as isize);
        }
        if m == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(h, Some(no_activate), id);
        }
        DefSubclassProc(h, m, w, l)
    }
}
pub fn activate(window: &gpui::Window) -> Result<()> {
    let h = hwnd(window)?;
    unsafe {
        let _ = ShowWindow(h, SW_RESTORE);
        SetForegroundWindow(h).ok()?;
        let _ = SetActiveWindow(h);
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(h));
    }
    Ok(())
}
pub fn show_popup(window: &gpui::Window, anchor: Rect, logical: [f64; 2]) -> Result<Rect> {
    let h = hwnd(window)?;
    unsafe {
        let style = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        SetLastError(WIN32_ERROR(0));
        if SetWindowLongPtrW(
            h,
            GWL_EXSTYLE,
            (style | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) as isize,
        ) == 0
            && GetLastError().0 != 0
        {
            return Err(Error::from_win32());
        }
        SetWindowSubclass(h, Some(no_activate), 1, 0).ok()?;
        let work = util::work_area([anchor.x as i32, anchor.y as i32]);
        SetWindowPos(
            h,
            Some(HWND_TOPMOST),
            work.x as i32,
            work.y as i32,
            1,
            1,
            SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )?;
        let scale = GetDpiForWindow(h).max(96) as f64 / 96.;
        let rect = anchor.clamp_popup(logical[0] * scale, logical[1] * scale, work);
        SetWindowPos(
            h,
            Some(HWND_TOPMOST),
            rect.x as i32,
            rect.y as i32,
            rect.width as i32,
            rect.height as i32,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )?;
        Ok(rect)
    }
}
pub fn move_popup(window: &gpui::Window, x: i32, y: i32) -> Result<()> {
    unsafe {
        let h = hwnd(window)?;
        let mut r = RECT::default();
        GetWindowRect(h, &mut r)?;
        let work = util::work_area([x, y]);
        let width = r.right - r.left;
        let height = r.bottom - r.top;
        SetWindowPos(
            h,
            None,
            x.max(work.x as i32)
                .min((work.x + work.width) as i32 - width),
            y.max(work.y as i32)
                .min((work.y + work.height) as i32 - height),
            0,
            0,
            SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
        )
    }
}
pub fn keep_accessible(window: &gpui::Window) -> Result<()> {
    unsafe {
        let h = hwnd(window)?;
        let mut rect = RECT::default();
        GetWindowRect(h, &mut rect)?;
        let work = util::work_area([(rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2]);
        let width = (rect.right - rect.left).min(work.width as i32);
        let height = (rect.bottom - rect.top).min(work.height as i32);
        let x = rect
            .left
            .max(work.x as i32)
            .min((work.x + work.width) as i32 - width);
        let y = rect
            .top
            .max(work.y as i32)
            .min((work.y + work.height) as i32 - height);
        SetWindowPos(h, None, x, y, width, height, SWP_NOACTIVATE | SWP_NOZORDER)
    }
}
fn icon(h: HWND, action: NOTIFY_ICON_MESSAGE) -> Result<()> {
    unsafe {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: h,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY,
            hIcon: LoadIconW(Some(GetModuleHandleW(None)?.into()), PCWSTR(1 as _))
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))?,
            ..Default::default()
        };
        let tip = util::wide("BeeK");
        data.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(action, &data).ok()
    }
}
fn configure(h: HWND) {
    unsafe {
        let _ = UnregisterHotKey(Some(h), 1);
        let (enabled, text) = HOTKEY.get().unwrap().lock().unwrap().clone();
        let parsed = parse_hotkey(&text);
        let ok = !enabled
            || parsed.is_some_and(|(mods, key)| {
                RegisterHotKey(Some(h), 1, mods | MOD_NOREPEAT, key).is_ok()
            });
        HOTKEY_OK.store(ok, Ordering::Release);
        CLIPBOARD_SEQUENCE.store(GetClipboardSequenceNumber(), Ordering::Release);
        let _ = RemoveClipboardFormatListener(h);
        if CLIPBOARD_ENABLED.load(Ordering::Acquire) {
            let _ = AddClipboardFormatListener(h);
        }
    }
}
pub fn parse_hotkey(text: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let mut mods = HOT_KEY_MODIFIERS(0);
    let mut key = None;
    for part in text.split('+') {
        match part.trim().to_ascii_lowercase().as_str() {
            "ctrl" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" => mods |= MOD_WIN,
            p if p.len() == 1 && p.as_bytes()[0].is_ascii_alphabetic() => {
                if key
                    .replace(p.as_bytes()[0].to_ascii_uppercase() as u32)
                    .is_some()
                {
                    return None;
                }
            }
            _ => return None,
        }
    }
    if mods.0 == 0 {
        return None;
    }
    key.map(|k| (mods, k))
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if m == RegisterWindowMessageW(w!("TaskbarCreated")) {
            let _ = icon(h, NIM_ADD);
            return LRESULT(0);
        }
        match m {
            TRAY if l.0 as u32 == WM_LBUTTONUP => send(Event::Settings),
            TRAY if l.0 as u32 == WM_RBUTTONUP => {
                if let Ok(menu) = CreatePopupMenu() {
                    let labels = [
                        w!("Settings"),
                        w!("Read clipboard..."),
                        w!("Restore selection detection"),
                        w!("About / updates"),
                        w!("Restart BeeK"),
                        w!("Quit BeeK"),
                    ];
                    for (i, label) in labels.into_iter().enumerate() {
                        let _ = AppendMenuW(menu, MF_STRING, i + 1, label);
                    }
                    let p = util::cursor();
                    let _ = SetForegroundWindow(h);
                    let selected = TrackPopupMenu(
                        menu,
                        TPM_RETURNCMD | TPM_NONOTIFY,
                        p[0],
                        p[1],
                        None,
                        h,
                        None,
                    )
                    .0;
                    match selected {
                        1 => send(Event::Settings),
                        2 => send(Event::Clipboard),
                        3 => send(Event::Restore),
                        4 => send(Event::About),
                        5 => send(Event::Restart),
                        6 => send(Event::Quit),
                        _ => {}
                    }
                    let _ = PostMessageW(Some(h), WM_NULL, WPARAM(0), LPARAM(0));
                    let _ = DestroyMenu(menu);
                }
            }
            WM_HOTKEY => send(Event::Hotkey),
            WM_CLIPBOARDUPDATE => {
                if CLIPBOARD_ENABLED.load(Ordering::Acquire) && {
                    let sequence = GetClipboardSequenceNumber();
                    sequence != 0 && CLIPBOARD_SEQUENCE.swap(sequence, Ordering::AcqRel) != sequence
                } {
                    send(Event::ClipboardChanged);
                }
            }
            WM_TIMER if w.0 == 2 => {
                if !SESSION_NOTIFICATIONS.load(Ordering::Acquire) {
                    SESSION_NOTIFICATIONS.store(
                        WTSRegisterSessionNotification(h, NOTIFY_FOR_THIS_SESSION).is_ok(),
                        Ordering::Release,
                    );
                }
                let active = !LOCKED.load(Ordering::Acquire) && util::desktop_available();
                if DESKTOP_ACTIVE.swap(active, Ordering::AcqRel) != active {
                    SYSTEM_EPOCH.fetch_add(1, Ordering::AcqRel);
                    invalidate(active);
                }
            }
            WM_WTSSESSION_CHANGE => {
                match w.0 as u32 {
                    WTS_SESSION_LOCK | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => {
                        LOCKED.store(true, Ordering::Release)
                    }
                    WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => {
                        LOCKED.store(false, Ordering::Release)
                    }
                    _ => {}
                }
                DESKTOP_ACTIVE.store(
                    !LOCKED.load(Ordering::Acquire) && util::desktop_available(),
                    Ordering::Release,
                );
                SYSTEM_EPOCH.fetch_add(1, Ordering::AcqRel);
                invalidate(true);
            }
            WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
                SYSTEM_EPOCH.fetch_add(1, Ordering::AcqRel);
                invalidate(true);
            }
            WM_POWERBROADCAST => {
                if w.0 == 4 {
                    DESKTOP_ACTIVE.store(false, Ordering::Release);
                    SYSTEM_EPOCH.fetch_add(1, Ordering::AcqRel);
                    invalidate(false);
                } else if matches!(w.0, 6 | 7 | 18) {
                    DESKTOP_ACTIVE.store(
                        !LOCKED.load(Ordering::Acquire) && util::desktop_available(),
                        Ordering::Release,
                    );
                    SYSTEM_EPOCH.fetch_add(1, Ordering::AcqRel);
                    invalidate(true);
                }
                return LRESULT(1);
            }
            CONFIG => configure(h),
            WM_CLOSE => {
                let _ = DestroyWindow(h);
            }
            WM_DESTROY => {
                let _ = icon(h, NIM_DELETE);
                PostQuitMessage(0);
            }
            _ => return DefWindowProcW(h, m, w, l),
        }
        LRESULT(0)
    }
}
pub struct Desktop {
    window: usize,
    worker: Option<thread::JoinHandle<()>>,
}
impl Desktop {
    pub fn start(events: mpsc::SyncSender<Event>) -> std::result::Result<Self, String> {
        MAIN_STARTED.store(true, Ordering::Release);
        DESKTOP_ACTIVE.store(util::desktop_available(), Ordering::Release);
        EVENTS.set(events).map_err(|_| "桌面服务已启动")?;
        HOTKEY.get_or_init(|| Mutex::new((false, String::new())));
        CLOCK.get_or_init(Instant::now);
        let (tx, rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || unsafe {
            let init = (|| -> Result<_> {
                let instance = GetModuleHandleW(None)?;
                let class = WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: instance.into(),
                    lpszClassName: w!("BeeKDesktopOwner"),
                    ..Default::default()
                };
                if RegisterClassW(&class) == 0 {
                    return Err(Error::from_win32());
                }
                let h = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("BeeKDesktopOwner"),
                    w!("BeeK desktop owner"),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                )?;
                icon(h, NIM_ADD)?;
                SESSION_NOTIFICATIONS.store(
                    WTSRegisterSessionNotification(h, NOTIFY_FOR_THIS_SESSION).is_ok(),
                    Ordering::Release,
                );
                SetTimer(Some(h), 2, 250, None);
                let mh = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), Some(instance.into()), 0)?;
                let kh =
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), Some(instance.into()), 0)?;
                let foreground = SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                let focus = SetWinEventHook(
                    EVENT_OBJECT_FOCUS,
                    EVENT_OBJECT_FOCUS,
                    None,
                    Some(changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                let selection = SetWinEventHook(
                    EVENT_OBJECT_TEXTSELECTIONCHANGED,
                    EVENT_OBJECT_TEXTSELECTIONCHANGED,
                    None,
                    Some(changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                let location = SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    None,
                    Some(changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                Ok((h, mh, kh, [foreground, focus, selection, location]))
            })();
            match init {
                Ok((h, mh, kh, hooks)) => {
                    let _ = tx.send(Ok(h.0 as usize));
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    let _ = UnhookWindowsHookEx(mh);
                    let _ = UnhookWindowsHookEx(kh);
                    for hook in hooks {
                        if !hook.is_invalid() {
                            let _ = UnhookWinEvent(hook);
                        }
                    }
                    let _ = UnregisterHotKey(Some(h), 1);
                    let _ = KillTimer(Some(h), 2);
                    let _ = WTSUnRegisterSessionNotification(h);
                    let _ = RemoveClipboardFormatListener(h);
                    let _ = icon(h, NIM_DELETE);
                }
                Err(_) => {
                    let _ = tx.send(Err("无法启动通知区或输入监听".to_owned()));
                }
            }
        });
        let window = rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "桌面服务启动超时")??;
        Ok(Self {
            window,
            worker: Some(worker),
        })
    }
    pub fn configure(&self, enabled: bool, hotkey: &str, clipboard: bool) {
        *HOTKEY.get().unwrap().lock().unwrap() = (enabled, hotkey.into());
        CLIPBOARD_ENABLED.store(clipboard, Ordering::Release);
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.window as *mut _)),
                CONFIG,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.window as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        let at = Instant::now();
        while self.worker.as_ref().is_some_and(|w| !w.is_finished())
            && at.elapsed() < Duration::from_millis(500)
        {
            thread::sleep(Duration::from_millis(5));
        }
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            let _ = self.worker.take().unwrap().join();
        }
    }
}

```
