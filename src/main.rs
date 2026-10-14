// BeeK v1.0 — GPUI 重写版
// 选中文本 → 小蜜蜂图标 → 点击翻译 → 弹窗显示（流式增量）
mod ax;
mod config;
mod icons;
mod menubar;
mod translate;

use ax::{ax_selected_text, ax_selection_bounds, is_url, SelBounds};
use gpui::*;
use icons::IconKind;
use std::sync::mpsc;
use std::time::{Duration, Instant};

// ---------- 事件 ----------
enum UiEvent {
    ShowIcon(SelBounds, String),
    HideIcon,
    /// 点击图标：发起翻译。请求 id 由主循环分配，用于丢弃过期结果。
    StartTranslate(String, SelBounds),
    /// 流式增量
    TranslateChunk(u64, String),
    /// 翻译结束
    TranslateDone(u64, Result<String, String>),
    ClosePopup,
}

/// 进程启动至今的毫秒数，用于日志取证
fn ms(t0: Instant) -> u128 {
    t0.elapsed().as_millis()
}

// ---------- 图标视图 ----------
struct IconView {
    text: String,
    bounds: SelBounds,
    loading: bool,
    hovered: bool,
    pressed: bool,
    /// 上一次绘制用的图标状态，仅用于日志取证（状态切换时才打印）
    last_kind: Option<IconKind>,
    tx: mpsc::Sender<UiEvent>,
}

impl Render for IconView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 用 listener 拿到 &mut self：点击当刻就置位 loading。
        // 如果只靠主循环轮询改状态，双击的第二下会漏进来，发出第二次翻译请求。
        let on_mouse_down = cx.listener(
            |this: &mut IconView,
             ev: &MouseDownEvent,
             _window: &mut Window,
             cx: &mut Context<IconView>| {
                if this.loading || ev.click_count > 1 {
                    return;
                }
                this.loading = true;
                this.pressed = true;
                cx.notify();
                let _ = this.tx.send(UiEvent::StartTranslate(this.text.clone(), this.bounds));
            },
        );
        let on_mouse_up = cx.listener(
            |this: &mut IconView,
             _ev: &MouseUpEvent,
             _window: &mut Window,
             cx: &mut Context<IconView>| {
                if this.pressed {
                    this.pressed = false;
                    cx.notify();
                }
            },
        );
        // hover 需要元素带 id 才会注册命中盒
        let on_hover = cx.listener(
            |this: &mut IconView,
             hovered: &bool,
             _window: &mut Window,
             cx: &mut Context<IconView>| {
                if this.hovered != *hovered {
                    this.hovered = *hovered;
                    cx.notify();
                }
            },
        );

        let kind = if self.loading {
            IconKind::Disabled
        } else if self.pressed {
            IconKind::Pressed
        } else if self.hovered {
            IconKind::Hover
        } else {
            IconKind::Default
        };
        if self.last_kind != Some(kind) {
            eprintln!("[beek] 图标状态切换 → {:?}", kind);
            self.last_kind = Some(kind);
        }

        // 素材本身是圆形徽标（四角透明），直接铺图标，不再衬白底卡片和圆角阴影
        div()
            .id("beek-icon")
            .w(px(ICON_SIZE))
            .h(px(ICON_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(
                img(kind.render_image(cx))
                    .w(px(icons::SELECTION_ICON_PT))
                    .h(px(icons::SELECTION_ICON_PT)),
            )
            .on_mouse_down(MouseButton::Left, on_mouse_down)
            .on_mouse_up(MouseButton::Left, on_mouse_up)
            .on_hover(on_hover)
    }
}

// ---------- 弹窗视图 ----------
enum PopupContent {
    Loading,
    Result(String),
    Error(String),
}

struct PopupView {
    text: String,
    content: PopupContent,
    tx: mpsc::Sender<UiEvent>,
}

impl Render for PopupView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.content {
            PopupContent::Loading => div().text_base().text_color(rgb(0x888888)).child("翻译中…"),
            PopupContent::Result(r) => div().text_base().child(r.clone()),
            PopupContent::Error(e) => div()
                .text_base()
                .text_color(rgb(0xcc3333))
                .child(format!("翻译失败：{}", e)),
        };
        // 点击当刻直接同步关窗，不等主循环下一轮轮询；
        // 同时通知主循环作废在飞行中的翻译，否则迟到的结果会把弹窗再弹回来一次。
        let on_mouse_down = cx.listener(
            |this: &mut PopupView,
             _ev: &MouseDownEvent,
             window: &mut Window,
             _cx: &mut Context<PopupView>| {
                let _ = this.tx.send(UiEvent::ClosePopup);
                window.remove_window();
            },
        );
        div()
            .id("popup")
            .w(px(POPUP_W))
            .h_full()
            .overflow_y_scroll()
            .p(px(16.))
            .bg(white())
            .rounded(px(12.))
            .shadow_lg()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, on_mouse_down)
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
/// 弹窗尺寸（点）
const POPUP_W: f32 = 380.;
const POPUP_H: f32 = 320.;

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
                hovered: false,
                pressed: false,
                last_kind: None,
                tx,
            })
        },
    )
    .ok()
}

fn open_popup_window(
    cx: &mut AsyncApp,
    text: String,
    content: PopupContent,
    b: SelBounds,
    tx: mpsc::Sender<UiEvent>,
) -> Option<WindowHandle<PopupView>> {
    let (dw, dh) = display_size(cx);
    let (sx, sy, _sw, sh) = gpui_rect(b, dh);
    let x = sx.clamp(SCREEN_MARGIN, (dw - POPUP_W - SCREEN_MARGIN).max(SCREEN_MARGIN));
    let y = (sy + sh + SCREEN_MARGIN).clamp(SCREEN_MARGIN, (dh - POPUP_H - SCREEN_MARGIN).max(SCREEN_MARGIN));
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(x), px(y)),
                size: size(px(POPUP_W), px(POPUP_H)),
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
                }
            }
        }
    });
}

// ---------- 命令行自检（BEEK_TRANSLATE_SELFTEST=1）----------
/// 不需要辅助功能权限、不需要图形界面，直接量接口耗时。
/// 用来验证「关思维链 + 流式」到底把等待时间压到了多少。
fn translate_selftest() {
    const SAMPLE: &str = "The quick brown fox jumps over the lazy dog. This sentence is long enough to show how fast the first characters arrive.";
    eprintln!("[自检] 文本 {} 字符", SAMPLE.chars().count());

    let t0 = Instant::now();
    match translate::translate(SAMPLE) {
        Ok(s) => eprintln!("[自检] 非流式：总计 {}ms → {}", ms(t0), s),
        Err(e) => eprintln!("[自检] 非流式失败：{}", e),
    }

    let t1 = Instant::now();
    let mut first: Option<u128> = None;
    let result = translate::translate_stream(SAMPLE, |chunk| {
        if first.is_none() {
            first = Some(ms(t1));
            eprintln!("[自检] 流式首字 {}ms：{:?}", ms(t1), chunk);
        }
    });
    eprintln!(
        "[自检] 流式：首字 {}ms，总计 {}ms → {}",
        match first {
            Some(v) => format!("{}", v),
            None => "无内容".to_string(),
        },
        ms(t1),
        match result {
            Ok(s) => s,
            Err(e) => format!("失败：{}", e),
        }
    );
}

// ---------- 自检（BEEK_SELFTEST=1）----------
/// 无需辅助功能权限，直接注入模拟选区走完整链路，
/// 用于验证「蜜蜂图标窗口 → 翻译请求 → 结果弹窗」这条链路真的能跑通。
fn spawn_selftest(tx: mpsc::Sender<UiEvent>, orig_show_in_dock: bool) {
    std::thread::spawn(move || {
        const SAMPLE: &str = "The quick brown fox jumps over the lazy dog.";
        // 回归用例用：足够长，保证「关闭弹窗」发生在结果返回之前
        const LONG_SAMPLE: &str =
            "Selection translation tools usually live in the menu bar and wait for the user \
             to highlight a piece of text. The hard part is not the translation itself, but \
             making the interface feel instant: the icon has to appear next to the selection \
             within a few hundred milliseconds, and the result window must open right away \
             instead of blocking on the network request. When the request is still in flight \
             and the user dismisses the window, a late response must never bring the window \
             back, otherwise the whole tool feels haunted by ghosts of previous requests.";
        let b = SelBounds { x: 600.0, y: 400.0, w: 220.0, h: 20.0 };
        std::thread::sleep(Duration::from_secs(2));
        eprintln!("[自检] 注入模拟选区（Cocoa 600,400 220x20）→ 显示蜜蜂图标");
        let _ = tx.send(UiEvent::ShowIcon(b, SAMPLE.to_string()));

        std::thread::sleep(Duration::from_secs(2));
        eprintln!("[自检] 模拟点击蜜蜂图标 → 发起翻译（弹窗应立即出现，内容流式填充）");
        let _ = tx.send(UiEvent::StartTranslate(SAMPLE.to_string(), b));

        std::thread::sleep(Duration::from_secs(1));
        eprintln!(
            "[自检] 点击后 1 秒：可见翻译弹窗 {} 个（期望 1，说明弹窗不依赖翻译完成）",
            menubar::popup_window_count()
        );
        menubar::dump_windows("自检：点击图标 1 秒后");

        // 回归用例：翻译「还在请求中」就关掉弹窗，迟到的结果不能把弹窗再弹回来。
        // 用长文本，并等上一条翻译彻底结束，保证关闭动作确实发生在收到结果之前。
        std::thread::sleep(Duration::from_secs(5));
        eprintln!("[自检] 回归：再次发起翻译（长文本），400ms 后关闭弹窗");
        let _ = tx.send(UiEvent::StartTranslate(LONG_SAMPLE.to_string(), b));
        std::thread::sleep(Duration::from_millis(400));
        eprintln!("[自检] 模拟点击弹窗关闭（此时请求仍在飞行中）");
        let _ = tx.send(UiEvent::ClosePopup);

        std::thread::sleep(Duration::from_secs(10));
        eprintln!(
            "[自检] 关闭后 10 秒：可见翻译弹窗 {} 个（期望 0，迟到的结果不得重新弹窗）",
            menubar::popup_window_count()
        );
        menubar::dump_windows("自检：请求途中关闭弹窗 10 秒后");
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
    if std::env::var("BEEK_TRANSLATE_SELFTEST").is_ok() {
        translate_selftest();
        return;
    }

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

        cx.spawn(async move |cx| {
            let t0 = Instant::now();
            let mut icon_win: Option<WindowHandle<IconView>> = None;
            let mut popup_win: Option<WindowHandle<PopupView>> = None;
            // 请求 id 记账：只认「当前在飞行中 / 当前展示」的那一次翻译。
            // 迟到的结果一律丢弃，否则关闭弹窗后它会被再弹回来。
            let mut req_seq: u64 = 0;
            let mut in_flight: Option<u64> = None;
            let mut shown: Option<u64> = None;
            // 已经写进弹窗的增量段数（自检取证用）
            let mut chunks_applied: u64 = 0;
            loop {
                while let Ok(ev) = rx.try_recv() {
                    match ev {
                        UiEvent::ShowIcon(b, text) => {
                            // 新选区：旧弹窗关闭，作废旧翻译
                            shown = None;
                            in_flight = None;
                            close_window(cx, &mut popup_win);
                            if icon_win.is_none() {
                                icon_win = open_icon_window(cx, b, text, tx.clone());
                            }
                        }
                        UiEvent::HideIcon => {
                            close_window(cx, &mut icon_win);
                        }
                        UiEvent::StartTranslate(text, b) => {
                            if let Some(id) = in_flight {
                                eprintln!("[beek] +{}ms 忽略重复点击（id={} 仍在翻译中）", ms(t0), id);
                                continue;
                            }
                            req_seq += 1;
                            let id = req_seq;
                            in_flight = Some(id);
                            shown = Some(id);
                            chunks_applied = 0;
                            close_window(cx, &mut icon_win);
                            close_window(cx, &mut popup_win);
                            // 关键：先开弹窗再等网络，用户点完立刻就有东西出来
                            popup_win = open_popup_window(
                                cx,
                                text.clone(),
                                PopupContent::Loading,
                                b,
                                tx.clone(),
                            );
                            eprintln!("[beek] +{}ms 点击图标 → 弹窗立即显示（id={}）", ms(t0), id);
                            let tx2 = tx.clone();
                            std::thread::spawn(move || {
                                let started = Instant::now();
                                let mut first_at: Option<u128> = None;
                                let result = translate::translate_stream(&text, |chunk| {
                                    if first_at.is_none() {
                                        first_at = Some(started.elapsed().as_millis());
                                    }
                                    let _ = tx2.send(UiEvent::TranslateChunk(id, chunk.to_string()));
                                });
                                let _ = tx2.send(UiEvent::TranslateDone(id, result));
                                eprintln!(
                                    "[beek] +{}ms 翻译线程结束（id={}，首字 {}ms，总 {}ms）",
                                    ms(t0),
                                    id,
                                    match first_at {
                                        Some(v) => format!("{}", v),
                                        None => "无".to_string(),
                                    },
                                    started.elapsed().as_millis()
                                );
                            });
                        }
                        UiEvent::TranslateChunk(id, chunk) => {
                            if Some(id) != shown {
                                continue;
                            }
                            if let Some(w) = &popup_win {
                                let ok = w
                                    .update(cx, |view, _window, vcx| {
                                        match &mut view.content {
                                            PopupContent::Result(s) => s.push_str(&chunk),
                                            _ => view.content = PopupContent::Result(chunk),
                                        }
                                        vcx.notify();
                                    })
                                    .is_ok();
                                if ok {
                                    chunks_applied += 1;
                                }
                            }
                        }
                        UiEvent::TranslateDone(id, result) => {
                            if in_flight == Some(id) {
                                in_flight = None;
                            }
                            if Some(id) != shown {
                                eprintln!("[beek] +{}ms 丢弃过期翻译结果（id={}）", ms(t0), id);
                                continue;
                            }
                            let mut applied = false;
                            if let Some(w) = &popup_win {
                                let content = match result {
                                    Ok(r) => PopupContent::Result(r),
                                    Err(e) => PopupContent::Error(e),
                                };
                                applied = w
                                    .update(cx, |view, _window, vcx| {
                                        view.content = content;
                                        vcx.notify();
                                    })
                                    .is_ok();
                            }
                            if applied {
                                eprintln!(
                                    "[beek] +{}ms 译文写入弹窗（id={}，已流式写入 {} 段）",
                                    ms(t0),
                                    id,
                                    chunks_applied
                                );
                            } else {
                                eprintln!("[beek] +{}ms 弹窗已不在，结果丢弃（id={}）", ms(t0), id);
                            }
                        }
                        UiEvent::ClosePopup => {
                            let was = shown.take();
                            // 作废在飞行中的翻译：关闭之后不再被迟到的结果弹回来
                            in_flight = None;
                            close_window(cx, &mut popup_win);
                            eprintln!(
                                "[beek] +{}ms 关闭弹窗（id={:?}，已流式写入 {} 段）",
                                ms(t0),
                                was,
                                chunks_applied
                            );
                        }
                    }
                }
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
            }
        })
        .detach();
    });
}
