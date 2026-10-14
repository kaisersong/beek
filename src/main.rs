// BeeK v1.0 — GPUI 重写版
// 选中文本 → 小蜜蜂图标 → 点击翻译 → 弹窗显示
mod ax;
mod config;
mod menubar;
mod translate;

use ax::{ax_selected_text, ax_selection_bounds, is_url, SelBounds};
use gpui::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

// ---------- 事件 ----------
enum UiEvent {
    ShowIcon(SelBounds, String),
    HideIcon,
    StartTranslate(String, SelBounds),
    TranslateDone(String, Result<String, String>, SelBounds),
    ClosePopup,
}

// ---------- 图标视图 ----------
struct IconView {
    text: String,
    bounds: SelBounds,
    loading: bool,
    tx: mpsc::Sender<UiEvent>,
}

impl Render for IconView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let tx = self.tx.clone();
        let text = self.text.clone();
        let b = self.bounds;
        let loading = self.loading;
        div()
            .w(px(36.))
            .h(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .bg(white())
            .rounded(px(10.))
            .shadow_lg()
            .cursor_pointer()
            .text_size(px(18.))
            .child(if loading { "⏳" } else { "🐝" })
            .on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                if !loading {
                    let _ = tx.send(UiEvent::StartTranslate(text.clone(), b));
                }
                cx.stop_propagation();
            })
    }
}

// ---------- 弹窗视图 ----------
enum PopupContent {
    Result(String),
    Error(String),
}

struct PopupView {
    text: String,
    content: PopupContent,
    tx: mpsc::Sender<UiEvent>,
}

impl Render for PopupView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let tx = self.tx.clone();
        let body = match &self.content {
            PopupContent::Result(r) => div().text_base().child(r.clone()),
            PopupContent::Error(e) => div()
                .text_base()
                .text_color(rgb(0xcc3333))
                .child(format!("翻译失败：{}", e)),
        };
        div()
            .id("popup")
            .w(px(380.))
            .h_full()
            .overflow_y_scroll()
            .p(px(16.))
            .bg(white())
            .rounded(px(12.))
            .shadow_lg()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                let _ = tx.send(UiEvent::ClosePopup);
                cx.stop_propagation();
            })
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x999999))
                    .mb(px(8.))
                    .child(self.text.clone()),
            )
            .child(body)
            .child(
                div()
                    .mt(px(12.))
                    .text_xs()
                    .text_color(rgb(0xbbbbbb))
                    .child("点击任意处关闭"),
            )
    }
}

// ---------- 窗口管理 ----------
/// 图标窗口边长（点）
const ICON_SIZE: f32 = 36.;
/// 图标与选区边缘的间距（点）
const ICON_GAP: f32 = 6.;
/// 窗口距离屏幕边缘的最小留白（点）
const SCREEN_MARGIN: f32 = 8.;

fn display_size(cx: &mut AsyncApp) -> (f32, f32) {
    cx.update(|app| {
        app.primary_display()
            .map(|d| {
                let b = d.bounds();
                (f32::from(b.size.width), f32::from(b.size.height))
            })
            .unwrap_or((1920., 1080.))
    })
    .unwrap_or((1920., 1080.))
}

/// AX 返回 Cocoa 屏幕坐标（左下原点），GPUI 的窗口坐标以显示器左上角为原点、y 轴向下。
/// 返回 (left, top, width, height)，单位为逻辑点。
fn gpui_rect(b: SelBounds, display_h: f32) -> (f32, f32, f32, f32) {
    let top = display_h - (b.y as f32 + b.h as f32);
    (b.x as f32, top, b.w as f32, b.h as f32)
}

fn close_window<V: Render>(cx: &mut AsyncApp, slot: &mut Option<WindowHandle<V>>) {
    if let Some(w) = slot.take() {
        let _ = w.update(cx, |_view, window, _vcx| window.remove_window());
    }
}

fn open_icon_window(
    cx: &mut AsyncApp,
    b: SelBounds,
    text: String,
    tx: mpsc::Sender<UiEvent>,
) -> Option<WindowHandle<IconView>> {
    let (dw, dh) = display_size(cx);
    let (sx, sy, sw, sh) = gpui_rect(b, dh);
    // 右边缘与选区右边缘对齐
    let x = (sx + sw - ICON_SIZE).clamp(SCREEN_MARGIN, (dw - ICON_SIZE - SCREEN_MARGIN).max(SCREEN_MARGIN));
    // 优先放在选区正上方；贴到屏幕上边缘时退到选区下方
    let above = sy - ICON_SIZE - ICON_GAP;
    let (y, side) = if above >= SCREEN_MARGIN {
        (above, "上方")
    } else {
        (
            (sy + sh + ICON_GAP).clamp(SCREEN_MARGIN, (dh - ICON_SIZE - SCREEN_MARGIN).max(SCREEN_MARGIN)),
            "下方",
        )
    };
    eprintln!(
        "[beek] 图标窗口位置：({:.0}, {:.0}) 选区{}，右边缘对齐（显示区 {:.0}x{:.0}）",
        x, y, side, dw, dh
    );
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(x), px(y)),
                size: size(px(ICON_SIZE), px(ICON_SIZE)),
            })),
            titlebar: None,
            kind: WindowKind::PopUp,
            focus: false,
            show: true,
            is_movable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        },
        |_window, cx| {
            cx.new(|_cx| IconView {
                text,
                bounds: b,
                loading: false,
                tx,
            })
        },
    )
    .ok()
}

fn open_popup_window(
    cx: &mut AsyncApp,
    text: String,
    result: Result<String, String>,
    b: SelBounds,
    tx: mpsc::Sender<UiEvent>,
) -> Option<WindowHandle<PopupView>> {
    let (dw, dh) = display_size(cx);
    let (sx, sy, _sw, sh) = gpui_rect(b, dh);
    let x = sx.clamp(8., (dw - 396.).max(8.));
    let y = (sy + sh + 8.).clamp(8., (dh - 336.).max(8.));
    let content = match result {
        Ok(r) => PopupContent::Result(r),
        Err(e) => PopupContent::Error(e),
    };
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(x), px(y)),
                size: size(px(380.), px(320.)),
            })),
            titlebar: None,
            kind: WindowKind::PopUp,
            focus: false,
            show: true,
            is_movable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        },
        |_window, cx| cx.new(|_cx| PopupView { text, content, tx }),
    )
    .ok()
}

// ---------- AX 监控后台线程 ----------
fn spawn_ax_monitor(tx: mpsc::Sender<UiEvent>) {
    std::thread::spawn(move || {
        let debug = std::env::var("BEEK_DEBUG").is_ok();
        let mut trusted = unsafe { accessibility_sys::AXIsProcessTrusted() };
        let mut ticks: u64 = 0;
        let mut last = String::new();
        let mut stable_since: Option<Instant> = None;
        let mut icon_shown = false;
        let mut dismissed = String::new();
        loop {
            std::thread::sleep(Duration::from_millis(50));
            ticks += 1;
            if !trusted {
                // 未授权时不做无意义的 AX 轮询；每 2 秒复查一次授权状态
                if ticks % 40 == 0 {
                    trusted = unsafe { accessibility_sys::AXIsProcessTrusted() };
                    if trusted {
                        eprintln!(
                            "[beek] 辅助功能权限已生效：开始取词（若选中文字仍无反应，请点菜单栏「重新启动 BeeK」）"
                        );
                    }
                }
                continue;
            }
            let cur = unsafe { ax_selected_text() };
            match cur {
                Some(t) if t == dismissed => {
                    if icon_shown {
                        let _ = tx.send(UiEvent::HideIcon);
                        icon_shown = false;
                    }
                    last.clear();
                    stable_since = None;
                }
                Some(t) if is_url(&t) => {
                    if icon_shown {
                        let _ = tx.send(UiEvent::HideIcon);
                        icon_shown = false;
                    }
                    last.clear();
                    stable_since = None;
                }
                Some(t) => {
                    if t != last {
                        if debug {
                            eprintln!("[beek] 新选区: {:?}", t);
                        }
                        last = t;
                        stable_since = Some(Instant::now());
                        if icon_shown {
                            let _ = tx.send(UiEvent::HideIcon);
                            icon_shown = false;
                        }
                    } else if let Some(since) = stable_since {
                        if !icon_shown && since.elapsed() >= Duration::from_millis(300) {
                            if let Some(b) = unsafe { ax_selection_bounds() } {
                                let _ = tx.send(UiEvent::ShowIcon(b, last.clone()));
                                if debug {
                                    eprintln!("[beek] 选区矩形(Cocoa): x={} y={} w={} h={}", b.x, b.y, b.w, b.h);
                                }
                                icon_shown = true;
                                stable_since = None;
                            } else if since.elapsed() >= Duration::from_millis(3000) {
                                // 3 秒都取不到矩形就不再重试，避免每个 tick 反复查 AX
                                if debug {
                                    eprintln!("[beek] 选区矩形取不到，跳过本次选区：{:?}", last);
                                }
                                stable_since = None;
                            }
                        }
                    }
                }
                None => {
                    if icon_shown {
                        let _ = tx.send(UiEvent::HideIcon);
                        icon_shown = false;
                    }
                    last.clear();
                    stable_since = None;
                    dismissed.clear();
                }
            }
        }
    });
}

// ---------- 自检（BEEK_SELFTEST=1）----------
/// 无需辅助功能权限，直接注入模拟选区走完整链路，
/// 用于验证「蜜蜂图标窗口 → 翻译请求 → 结果弹窗」这条链路真的能跑通。
fn spawn_selftest(tx: mpsc::Sender<UiEvent>, orig_show_in_dock: bool) {
    std::thread::spawn(move || {
        const SAMPLE: &str = "The quick brown fox jumps over the lazy dog.";
        let b = SelBounds { x: 600.0, y: 400.0, w: 220.0, h: 20.0 };
        std::thread::sleep(Duration::from_secs(2));
        eprintln!("[自检] 注入模拟选区（Cocoa 600,400 220x20）→ 显示蜜蜂图标");
        let _ = tx.send(UiEvent::ShowIcon(b, SAMPLE.to_string()));

        std::thread::sleep(Duration::from_secs(2));
        eprintln!("[自检] 模拟点击蜜蜂图标 → 发起翻译");
        let _ = tx.send(UiEvent::StartTranslate(SAMPLE.to_string(), b));

        std::thread::sleep(Duration::from_secs(8));
        menubar::dump_windows("自检：翻译弹窗阶段");
        menubar::dump_menu();

        eprintln!(
            "[自检] 点击菜单项前：activationPolicy={} 勾选={}",
            menubar::activation_policy(),
            menubar::toggle_item_state()
        );
        let p1 = menubar::trigger_dock_toggle();
        eprintln!(
            "[自检] 第一次点击「在 Dock 中显示」后：activationPolicy={}（0=显示 1=隐藏）勾选={}",
            p1,
            menubar::toggle_item_state()
        );
        std::thread::sleep(Duration::from_secs(1));
        let p2 = menubar::trigger_dock_toggle();
        eprintln!(
            "[自检] 第二次点击「在 Dock 中显示」后：activationPolicy={}（0=显示 1=隐藏）勾选={}",
            p2,
            menubar::toggle_item_state()
        );

        // 恢复用户原本的设置（自检不应该改动用户配置）
        if config::load().show_in_dock != orig_show_in_dock {
            config::save(&config::Config { show_in_dock: orig_show_in_dock });
            menubar::set_dock_visible(orig_show_in_dock);
        }
        eprintln!("[自检] 完成，show_in_dock 已恢复为 {}", orig_show_in_dock);
    });
}

// ---------- 主入口 ----------
fn main() {
    Application::new().run(|cx: &mut App| {
        let cfg = config::load();
        menubar::set_dock_visible(cfg.show_in_dock);
        menubar::setup_status_item(&cfg);

        let trusted = unsafe { accessibility_sys::AXIsProcessTrusted() };
        eprintln!(
            "[beek] 辅助功能权限：{}",
            if trusted { "已授权，取词可用" } else { "未授权，取词不可用" }
        );
        if !trusted {
            // 弹系统对话框，引导用户直接去勾选 BeeK
            unsafe { ax::prompt_accessibility() };
        }

        let (tx, rx) = mpsc::channel::<UiEvent>();
        if std::env::var("BEEK_SELFTEST").is_ok() {
            spawn_selftest(tx.clone(), cfg.show_in_dock);
        }
        spawn_ax_monitor(tx.clone());

        let debug_events = std::env::var("BEEK_DEBUG").is_ok();
        cx.spawn(async move |cx| {
            let mut icon_win: Option<WindowHandle<IconView>> = None;
            let mut popup_win: Option<WindowHandle<PopupView>> = None;
            loop {
                while let Ok(ev) = rx.try_recv() {
                    match ev {
                        UiEvent::ShowIcon(b, text) => {
                            // 新选区出现时，旧弹窗关闭，图标取而代之
                            close_window(cx, &mut popup_win);
                            if icon_win.is_none() {
                                icon_win = open_icon_window(cx, b, text, tx.clone());
                            }
                        }
                        UiEvent::HideIcon => {
                            close_window(cx, &mut icon_win);
                        }
                        UiEvent::StartTranslate(text, b) => {
                            if let Some(w) = &icon_win {
                                let _ = w.update(cx, |view, _window, vcx| {
                                    view.loading = true;
                                    vcx.notify();
                                });
                            }
                            let tx2 = tx.clone();
                            std::thread::spawn(move || {
                                let result = translate::translate(&text);
                                let _ = tx2.send(UiEvent::TranslateDone(text, result, b));
                            });
                        }
                        UiEvent::TranslateDone(text, result, b) => {
                            if debug_events {
                                eprintln!("[beek] 翻译结果：{:?}", result);
                            }
                            close_window(cx, &mut icon_win);
                            close_window(cx, &mut popup_win);
                            popup_win = open_popup_window(cx, text, result, b, tx.clone());
                        }
                        UiEvent::ClosePopup => {
                            close_window(cx, &mut popup_win);
                        }
                    }
                }
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
            }
        })
        .detach();
    });
}
