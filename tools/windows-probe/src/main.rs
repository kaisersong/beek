//! Independent G0 experiment, not a Windows BeeK product implementation.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod input;
#[cfg(windows)]
mod native;
mod text_model;

use gpui::{prelude::*, *};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::Write,
    sync::{Mutex, OnceLock, mpsc},
    time::{Duration, Instant},
};

static LOG: OnceLock<Mutex<File>> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();

fn record(event: &str, data: Value) {
    // No selected text, typed characters, API keys or clipboard content.
    let line = json!({"schema":2,"ms":START.get_or_init(Instant::now).elapsed().as_millis(),
        "event":event,"data":data});
    if let Some(log) = LOG.get() {
        if let Ok(mut file) = log.lock() {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}

#[derive(Clone, Copy)]
#[cfg_attr(not(windows), allow(dead_code))] // Native tray commands are Windows-only.
enum Command {
    Input,
    Overlay,
    ShowOverlay(u64),
    HideAll,
    Quit,
}
actions!(probe, [NextField, PreviousField]);

struct InputView {
    fields: [Entity<input::TextInput>; 2],
    focused: usize,
    tx: mpsc::SyncSender<Command>,
}
impl InputView {
    fn next(&mut self, _: &NextField, window: &mut Window, cx: &mut Context<Self>) {
        self.focused = if self.fields[0].focus_handle(cx).is_focused(window) {
            1
        } else {
            0
        };
        self.fields[self.focused].read(cx).focus(window);
        record("field_navigation", json!({"field": self.focused}));
    }
    fn previous(&mut self, _: &PreviousField, window: &mut Window, cx: &mut Context<Self>) {
        self.next(&NextField, window, cx);
    }
}
impl Render for InputView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .gap_4()
            .bg(rgb(0xf5f5f5))
            .text_color(rgb(0x242424))
            .on_action(cx.listener(Self::next))
            .on_action(cx.listener(Self::previous))
            .child("BeeK G0 · Windows input / IME")
            .child("仅输入公开测试文本：中文、emoji、组合输入、Ctrl+A/C/V/X、Tab 切换。")
            .child(self.fields[0].clone())
            .child(self.fields[1].clone())
            .child(
                div()
                    .id("show-overlay")
                    .p_2()
                    .bg(rgb(0xf6c64a))
                    .cursor_pointer()
                    .child("3秒后显示非激活透明窗（立即切回记事本测试）")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| {
                            let _ = this.tx.try_send(Command::Overlay);
                        }),
                    ),
            )
            .child("关闭所有窗口后，进程应继续驻留；从通知区重新打开。右键通知区图标退出。")
    }
}

struct Overlay {
    clicks: u32,
}

/// GPUI 0.2.2 Windows posts WM_QUIT when its last window is destroyed.
/// A permanent, never-shown lifecycle host keeps the GPUI dispatcher alive.
/// This is not a visible/transient popup and is released only on explicit quit.
struct LifetimeHost;
impl Render for LifetimeHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
impl Render for Overlay {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("probe-overlay")
            .size_full()
            .p_2()
            .rounded_xl()
            .bg(rgba(0xffc83dcc))
            .text_color(rgb(0x242424))
            .cursor_pointer()
            .child(format!("BeeK · 点击 {} 次", self.clicks))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.clicks += 1;
                    #[cfg(windows)]
                    record(
                        "overlay_clicked",
                        json!({"count":this.clicks,"foreground":native::foreground()}),
                    );
                    cx.notify();
                }),
            )
    }
}

fn open_input(cx: &mut App, tx: mpsc::SyncSender<Command>) -> WindowHandle<InputView> {
    cx.open_window(
        WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("BeeK G0 input / IME".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(720.), px(380.)),
                cx,
            ))),
            ..Default::default()
        },
        |window, cx| {
            let a = cx.new(|cx| input::TextInput::new(cx, "测试字段一 / 中文 IME"));
            let b = cx.new(|cx| input::TextInput::new(cx, "测试字段二 / Tab / 粘贴"));
            a.read(cx).focus(window);
            cx.new(|_| InputView {
                fields: [a, b],
                focused: 0,
                tx,
            })
        },
    )
    .expect("G0 input window creation failed")
}

fn main() {
    if !cfg!(windows) {
        eprintln!(
            "G0 runtime acceptance requires Windows 11 x64; this host can only compile-check."
        );
    }
    if let Some(path) = std::env::args().skip(1).next() {
        // Explicit path supplied by the runner; never create an implicit user-text log.
        match File::create(path) {
            Ok(file) => {
                let _ = LOG.set(Mutex::new(file));
            }
            Err(e) => {
                eprintln!("cannot create G0 evidence log: {e}");
                std::process::exit(2);
            }
        }
    }
    START.get_or_init(Instant::now);
    record(
        "started",
        json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"gpui":"0.2.2"}),
    );
    let (tx, rx) = mpsc::sync_channel(16);
    #[cfg(windows)]
    let tray = native::Tray::start(tx.clone()).expect("G0 requires a working tray");
    Application::new().run(move |cx| {
        let lifetime_host = cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.),px(0.)),size(px(1.),px(1.))))),
            titlebar: None, focus: false, show: false, kind: WindowKind::PopUp,
            is_movable: false, is_resizable: false,
            ..Default::default()
        }, |_,cx| cx.new(|_| LifetimeHost)).expect("G0 lifecycle host creation failed");
        #[cfg(windows)]
        lifetime_host.update(cx, |_,window,_| native::record_lifetime_host(window)).expect("G0 lifecycle host invalid");
        input::bind(cx);
        cx.bind_keys([KeyBinding::new("tab", NextField, Some("TextInput")),
            KeyBinding::new("shift-tab", PreviousField, Some("TextInput"))]);
        let mut input_window = Some(open_input(cx,tx.clone()));
        let mut overlay: Option<WindowHandle<Overlay>> = None;
        cx.on_window_closed(|cx| record("window_closed", json!({"remaining_visible":cx.windows().len().saturating_sub(1)}))).detach();
        cx.spawn(async move |cx| {
            let _lifetime_host_guard = lifetime_host;
            #[cfg(windows)]
            let _tray_guard = tray;
            let mut last_heartbeat = Instant::now();
            let mut overlay_ticket = 0u64;
            loop {
                while let Ok(command) = rx.try_recv() {
                    let _ = cx.update(|cx| match command {
                        Command::Input => {
                            let active = input_window.and_then(|w| w.update(cx, |_, window, _| window.activate_window()).ok());
                            if active.is_none() { input_window = Some(open_input(cx, tx.clone())); }
                            record("input_opened", json!({}));
                        }
                        Command::Overlay => {
                            let delayed = tx.clone();
                            overlay_ticket = overlay_ticket.wrapping_add(1);
                            let ticket = overlay_ticket;
                            record("overlay_scheduled", json!({"delay_ms":3000}));
                            cx.spawn(async move |cx| {
                                cx.background_executor().timer(Duration::from_secs(3)).await;
                                let _ = delayed.try_send(Command::ShowOverlay(ticket));
                            }).detach();
                        }
                        Command::ShowOverlay(ticket) => {
                            if ticket != overlay_ticket { return; }
                            if let Some(old) = overlay.take() { let _ = old.update(cx, |_, w, _| w.remove_window()); }
                            #[cfg(windows)]
                            let before = native::foreground();
                            let w = cx.open_window(WindowOptions {
                                window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(24.),px(48.)),size(px(220.),px(76.))))),
                                titlebar:None, focus:false, show:false, kind:WindowKind::PopUp,
                                is_movable:false, is_resizable:false,
                                window_background:WindowBackgroundAppearance::Transparent,
                                ..Default::default()
                            }, |_,cx| cx.new(|_| Overlay{clicks:0})).expect("G0 overlay creation failed");
                            #[cfg(windows)]
                            w.update(cx, |_,window,_| {
                                if let Err(e) = native::show_overlay(window) {
                                    record("overlay_error", json!({"code":e.code().0}));
                                    window.remove_window();
                                }
                                record("overlay_shown", json!({"foreground_before":before,"foreground_after":native::foreground()}));
                            }).expect("G0 overlay handle invalid");
                            overlay = Some(w);
                        }
                        Command::HideAll => {
                            overlay_ticket = overlay_ticket.wrapping_add(1);
                            if let Some(old) = overlay.take() { let _ = old.update(cx, |_, w, _| w.remove_window()); }
                            if let Some(old) = input_window.take() { let _ = old.update(cx, |_, w, _| w.remove_window()); }
                            record("all_windows_closed", json!({}));
                        }
                        Command::Quit => { record("quit",json!({})); cx.quit(); }
                    });
                }
                if last_heartbeat.elapsed() >= Duration::from_secs(5) {
                    let _ = cx.update(|cx| record("heartbeat",json!({"visible_windows":cx.windows().len().saturating_sub(1),"gpui_windows":cx.windows().len(),"lifetime_hosts":1})));
                    last_heartbeat = Instant::now();
                }
                cx.background_executor().timer(Duration::from_millis(16)).await;
            }
        }).detach();
    });
}
