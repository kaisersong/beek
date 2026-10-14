// BeeK v1.0 — GPUI 重写版
// 选中文本 → 小蜜蜂图标 → 点击翻译 → 弹窗显示
mod ax;
mod translate;

use ax::{ax_selected_text, ax_selection_bounds, is_url, SelBounds};
use gpui::*;
use std::sync::mpsc;
use std::time::{Duration, Instant};

// ---------- 状态 ----------
#[derive(Clone)]
enum PopupState {
    Hidden,
    Loading { text: String, bounds: SelBounds },
    Result { text: String, result: String, bounds: SelBounds },
}

struct BeeK {
    popup: PopupState,
    tx: mpsc::Sender<UiEvent>,
}

enum UiEvent {
    ShowIcon(SelBounds, String),
    HideIcon,
    ShowLoading(String, SelBounds),
    ShowResult(String, String, SelBounds),
    HidePopup,
}

// ---------- 图标视图 ----------
struct IconView {
    bounds: SelBounds,
    text: String,
}

impl Render for IconView {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .size(px(24.))
            .child("🐝")
            .text_size(px(16.))
    }
}

// ---------- 弹窗视图 ----------
struct PopupView {
    state: PopupState,
}

impl Render for PopupView {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        match &self.state {
            PopupState::Hidden => div().into_any_element(),
            PopupState::Loading { text, .. } => {
                div()
                    .w(px(380.))
                    .p(px(16.))
                    .bg(white())
                    .rounded(px(12.))
                    .shadow_lg()
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x666666))
                            .child(format!("翻译中: {}", text))
                    )
                    .into_any_element()
            }
            PopupState::Result { text, result, .. } => {
                div()
                    .w(px(380.))
                    .p(px(16.))
                    .bg(white())
                    .rounded(px(12.))
                    .shadow_lg()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(text.clone())
                    )
                    .child(
                        div()
                            .mt(px(8.))
                            .text_base()
                            .child(result.clone())
                    )
                    .into_any_element()
            }
        }
    }
}

// ---------- AX 监控后台线程 ----------
fn spawn_ax_monitor(tx: mpsc::Sender<UiEvent>) {
    std::thread::spawn(move || {
        let mut last = String::new();
        let mut stable_since: Option<Instant> = None;
        let mut icon_shown = false;
        let mut dismissed = String::new();
        loop {
            std::thread::sleep(Duration::from_millis(50));
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
                        last = t;
                        stable_since = Some(Instant::now());
                        if icon_shown {
                            let _ = tx.send(UiEvent::HideIcon);
                            icon_shown = false;
                        }
                    } else if let Some(since) = stable_since {
                        if since.elapsed() >= Duration::from_millis(300) && !icon_shown {
                            if let Some(b) = unsafe { ax_selection_bounds() } {
                                let _ = tx.send(UiEvent::ShowIcon(b, last.clone()));
                                icon_shown = true;
                            }
                            stable_since = None;
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

// ---------- 主入口 ----------
fn main() {
    App::new().run(|cx: &mut AppContext| {
        let (tx, rx) = mpsc::channel::<UiEvent>();
        spawn_ax_monitor(tx.clone());
        let icon_win = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point { x: px(-100.), y: px(-100.) },
                    size: Size { width: px(24.), height: px(24.) },
                })),
                titlebar: None,
                window_decorations: Some(WindowDecorations::Client),
                ..Default::default()
            },
            |cx| cx.new_view(|_cx| IconView {
                bounds: SelBounds { x: 0., y: 0., w: 0., h: 0. },
                text: String::new(),
            }),
        ).unwrap();
        let popup_win = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point { x: px(-1000.), y: px(-1000.) },
                    size: Size { width: px(380.), height: px(200.) },
                })),
                titlebar: None,
                ..Default::default()
            },
            |cx| cx.new_view(|_cx| PopupView { state: PopupState::Hidden }),
        ).unwrap();
        cx.spawn(|mut cx| async move {
            while let Ok(ev) = rx.recv() {
                match ev {
                    UiEvent::ShowIcon(b, text) => {
                        let _ = icon_win.update(&mut cx, |view, cx| {
                            view.bounds = b;
                            view.text = text;
                            cx.notify();
                        });
                    }
                    UiEvent::HideIcon => {},
                    UiEvent::ShowLoading(text, b) => {
                        let _ = popup_win.update(&mut cx, |view, cx| {
                            view.state = PopupState::Loading { text, bounds: b };
                            cx.notify();
                        });
                    }
                    UiEvent::ShowResult(text, result, b) => {
                        let _ = popup_win.update(&mut cx, |view, cx| {
                            view.state = PopupState::Result { text, result, bounds: b };
                            cx.notify();
                        });
                    }
                    UiEvent::HidePopup => {
                        let _ = popup_win.update(&mut cx, |view, cx| {
                            view.state = PopupState::Hidden;
                            cx.notify();
                        });
                    }
                }
            }
        }).detach();
    });
}
