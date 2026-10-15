// BeeK v1.0 — GPUI 重写版
// 选中文本 → 小蜜蜂图标 → 点击翻译 → 弹窗显示（流式增量）
mod ax;
mod clipboard;
mod config;
mod dialogs;
mod icons;
mod menubar;
mod mouse;
mod translate;
mod update;

use ax::{ax_selection, is_url, SelBounds};
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
    /// 自检用：走真实复制逻辑把译文写进剪贴板
    CopyResult,
    /// 自检用：在主线程把弹窗挪到指定位置（AppKit 只能在主线程动窗口）
    DragTest(f64, f64),
    ClosePopup,
    // ---- 菜单栏动作（菜单在主线程，但开户/发请求的活儿交给主循环）----
    /// 「设置 API Key…」
    OpenKeySettings,
    /// 「关于 BeeK」
    OpenAbout,
    /// 「检查更新…」
    CheckUpdate,
    /// 检查更新的结果（None = 仓库还没发布过 release）
    UpdateChecked(Result<Option<update::Release>, String>),
    /// 升级包已下载并解压好：(解压出来的新 app, 版本号)
    UpdatePrepared(std::path::PathBuf, String),
    /// 更新过程中的失败（下载 / 解压 / 替换脚本）
    UpdateFailed(String),
}

/// 朗读一个词：用系统自带的 /usr/bin/say，不联网也不需要额外权限。
/// 中文（CJK）用「Tingting」音色，英文用「Samantha」；音色不可用时退回系统默认音色。
fn speak(word: &str) {
    let w = word.trim().to_string();
    if w.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let voice = if w.chars().any(|c| (c as u32) > 0x2e80) { "Tingting" } else { "Samantha" };
        let with_voice = std::process::Command::new("/usr/bin/say")
            .arg("-v")
            .arg(voice)
            .arg(&w)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if with_voice {
            return;
        }
        let fallback = std::process::Command::new("/usr/bin/say")
            .arg(&w)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if fallback {
            eprintln!("[beek] 朗读 {:?}：{} 音色不可用，已用系统默认音色", w, voice);
        } else {
            eprintln!("[beek] 朗读 {:?} 失败（/usr/bin/say 不可用）", w);
        }
    });
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

        // 素材是圆形徽标（四角透明），直接铺图标；放大 ICON_ZOOM 后由容器裁掉外圈描边。
        // 注意 img() 的尺寸不吃 pt 换算，必须用 ICON_SIZE * ICON_ZOOM（见 ICON_ZOOM 的推导）。
        div()
            .id("beek-icon")
            .w(px(ICON_SIZE))
            .h(px(ICON_SIZE))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(
                img(kind.render_image(cx))
                    .w(px(ICON_SIZE * ICON_ZOOM))
                    .h(px(ICON_SIZE * ICON_ZOOM)),
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
    content: PopupContent,
    /// Some(词) = 单词模式（词典式详解）：卡片顶部显示「词 + 读音 + 发音图标」
    word: Option<String>,
    /// true = 卡片贴窗口底部。弹窗放在选区上方时用，这样内容变长是向上长，不会压住选区
    anchor_bottom: bool,
    /// 窗口当前高度（卡片用它做高度上限，超出则内部滚动）
    win_h: f32,
    /// 「已复制」提示是否显示中
    copied: bool,
}

impl PopupView {
    /// 卡片里要显示的正文
    fn body_text(&self) -> String {
        match &self.content {
            PopupContent::Loading => {
                if self.word.is_some() {
                    "查询中…".to_string()
                } else {
                    "翻译中…".to_string()
                }
            }
            PopupContent::Result(t) => t.clone(),
            PopupContent::Error(e) => format!("翻译失败：{}", e),
        }
    }

    /// 单词模式：把「读音：…」那一行从正文里拆出来，返回 (读音, 不含读音行的正文)
    fn pron_and_body(&self) -> (Option<String>, String) {
        match &self.content {
            PopupContent::Result(raw) => translate::split_pron(raw),
            _ => (None, self.body_text()),
        }
    }

    /// 复制到剪贴板的内容：单词模式连词和读音一起复制，方便粘到别处
    fn copy_text(&self) -> String {
        let (pron, body) = self.pron_and_body();
        match (self.word.as_deref(), pron) {
            (Some(w), Some(p)) => format!("{}  {}\n{}", w, p, body),
            (Some(w), None) => format!("{}\n{}", w, body),
            _ => body,
        }
    }

    /// 复制正文到系统剪贴板，并短暂显示「已复制」
    fn copy(&mut self, cx: &mut Context<Self>) {
        if matches!(self.content, PopupContent::Loading) {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(self.copy_text()));
        self.copied = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for PopupView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 单词模式：正文里的「读音：…」那一行抽出来做成标题，不在正文里重复
        let (pron, shown) = self.pron_and_body();
        let body = div()
            .text_size(px(POPUP_FONT_PT))
            .text_color(if matches!(self.content, PopupContent::Error(_)) {
                rgb(0xcc3333)
            } else if matches!(self.content, PopupContent::Loading) {
                rgb(0x888888)
            } else {
                rgb(0x1f1f1f)
            })
            .child(shown);

        // 发音按钮：点一下用系统语音朗读这个词
        let on_speak = cx.listener(
            |this: &mut PopupView, _ev: &MouseDownEvent, _w: &mut Window, _cx: &mut Context<PopupView>| {
                if let Some(w) = this.word.clone() {
                    speak(&w);
                }
            },
        );
        // 顶部标题行（只有单词模式才有）：词 + 读音 + 发音图标
        // 发音图标紧贴在音标右边（原来甩在卡片最右端，离音标隔着半个卡片），
        // 并且是线描式的：彩色 emoji 跟灰色的音标不是一套视觉语言。
        let header = self.word.clone().map(|w| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .mb(px(8.))
                .child(div().text_lg().font_weight(FontWeight::BOLD).child(w))
                .child(match pron.clone() {
                    Some(p) => div().text_sm().text_color(rgb(0x666666)).child(p),
                    None => div().text_sm().text_color(rgb(0xcccccc)).child("读音…"),
                })
                .child(
                    div()
                        .id("beek-speak")
                        .w(px(icons::SPEAK_ICON_PT + 6.))
                        .h(px(icons::SPEAK_ICON_PT + 6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .hover(|s| s.bg(rgb(0xe8e8e8)))
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, on_speak)
                        .child(
                            img(icons::speak_outline(cx))
                                .w(px(icons::SPEAK_ICON_PT))
                                .h(px(icons::SPEAK_ICON_PT)),
                        ),
                )
        });

        // 弹窗内点击不做任何事：关闭动作是「点弹窗外部」，
        // 由主循环轮询全局鼠标状态完成（非激活窗口收不到外部点击事件）。
        let on_copy = cx.listener(
            |this: &mut PopupView, _ev: &MouseDownEvent, _w: &mut Window, cx: &mut Context<PopupView>| {
                this.copy(cx);
            },
        );

        let card = div()
            .id("beek-popup")
            .w(px(POPUP_W))
            .max_h(px(self.win_h))
            .overflow_y_scroll()
            .p(px(POPUP_PAD))
            .bg(white())
            .rounded(px(12.))
            .shadow_lg()
            // 顶部拖动条：等于 macOS 原生的窗口拖动区，按住可以拖走整个弹窗
            .child(
                div()
                    .id("beek-drag")
                    .w_full()
                    .h(px(POPUP_DRAG_H))
                    .mb(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().w(px(22.)).h(px(2.)).rounded(px(1.)).bg(rgb(0xcccccc)))
                            .child(div().w(px(22.)).h(px(2.)).rounded(px(1.)).bg(rgb(0xcccccc))),
                    ),
            )
            .children(header)
            .child(body)
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0xbbbbbb))
                            .child("点击弹窗外任意处关闭"),
                    )
                    .child(
                        // 复制按钮：两个叠起来的圆角方框 = 复制图标
                        div()
                            .id("beek-copy")
                            .w(px(20.))
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(5.))
                            .hover(|s| s.bg(rgb(0xe8e8e8)))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, on_copy)
                            .child(
                                div()
                                    .relative()
                                    .w(px(12.))
                                    .h(px(12.))
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px(4.))
                                            .top(px(0.))
                                            .w(px(8.))
                                            .h(px(8.))
                                            .border_1()
                                            .border_color(rgb(0xbbbbbb))
                                            .rounded(px(2.)),
                                    )
                                    .child(
                                        div()
                                            .absolute()
                                            .left(px(0.))
                                            .top(px(4.))
                                            .w(px(8.))
                                            .h(px(8.))
                                            .bg(white())
                                            .border_1()
                                            .border_color(if self.copied {
                                                rgb(0x2f9e44)
                                            } else {
                                                rgb(0x888888)
                                            })
                                            .rounded(px(2.)),
                                    ),
                            ),
                    ),
            );

        // 放在选区上方时把卡片贴到窗口底部：窗口顶端固定，高度变大时卡片向上长
        let mut root = div().w(px(POPUP_W)).h_full().flex().flex_col();
        if self.anchor_bottom {
            root = root.justify_end();
        }
        root.child(card)
    }
}

// ---------- 窗口管理 ----------
/// 图标窗口边长（点）= 悬浮按钮可见圆的直径。
/// 32pt → 16pt（半数）后用户反馈偏小，再放大 50%：16 × 1.5 = 24pt。
const ICON_SIZE: f32 = 24.;
/// 图标与选区边缘的间距（点）：0 = 紧贴选区上沿
const ICON_GAP: f32 = 0.;
/// 绘制时把素材放大一点再裁掉外圈，看起来就是「没有边框」。
///
/// 素材是 64x64 的圆形徽标，不透明的圆只占 x=3..61，也就是左右各 3/64 = 4.6875% 是
/// 透明/半透明的抗锯齿边，贴在浅色背景上就是用户看到的那圈「边框」。
/// 取 Z 使得「放大后被容器裁掉的边」正好等于这 4.6875%：
///   (1 - 1/Z) / 2 = 0.046875  →  Z = 1 / 0.90625 = 1.103448…
/// 于是容器的 16pt 正好等于完整的圆，边缘的抗锯齿被裁掉，圆本身一点不糊、也不被切。
const ICON_ZOOM: f32 = 1.103448;
/// 窗口距离屏幕边缘的最小留白（点）
const SCREEN_MARGIN: f32 = 8.;
/// 弹窗与选区的间距（点）
const POPUP_GAP: f32 = 8.;
/// 弹窗宽度（点）
const POPUP_W: f32 = 380.;
/// 弹窗内边距（点）
const POPUP_PAD: f32 = 16.;
/// 译文正文字号（pt）：对齐「备忘录」的标准正文大小（13pt）。
/// 之前用 text_base（1rem = 16pt）明显偏大，读一段译文像在看标题。
const POPUP_FONT_PT: f32 = 13.;
/// 弹窗高度上限（点）：内容超过就内部滚动
const POPUP_MAX_H: f32 = 320.;
/// 打开时的起始高度（点）：只用于决定放上方还是下方，随后立刻按内容量准
const POPUP_MIN_H: f32 = 92.;
/// 页脚高度（点）：一行提示 + 复制按钮
const POPUP_FOOTER_H: f32 = 22.;
/// 顶部拖动条高度（点）
const POPUP_DRAG_H: f32 = 12.;
/// 决定「放下方还是上方」时要求的最小可用高度（点）：
/// 只要下方还有这么多空间就优先放下方（不够才转上方）
const POPUP_MIN_USABLE: f32 = 80.;

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

/// AX 返回的就是「显示器左上角为原点、y 轴向下」的屏幕坐标，和 GPUI 的窗口坐标系一致，
/// 所以这里只做 f64→f32，**不能翻转 y**。
///
/// 实测（BEEK_AX_PROBE 探针，TextEdit）：文本区 AXPosition=(181,134)、AXSize=(656,384)，
/// 与窗口工具读到的同一元素 frame（左上原点）逐值相等；第一行 5 个字符的
/// AXBoundsForRange = (191, 134, 33.1, 13)，y=134 正是第一行的上沿。
/// 早期版本按「Cocoa 左下原点」翻转，导致图标和弹窗被镜像到选区上方近一个屏高的位置。
/// 返回 (left, top, width, height)，单位为逻辑点。
fn gpui_rect(b: SelBounds, _display_h: f32) -> (f32, f32, f32, f32) {
    (b.x as f32, b.y as f32, b.w as f32, b.h as f32)
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
        "[beek] 图标窗口位置：({:.0}, {:.0}) 选区{}，右边缘对齐（选区=({:.0}, {:.0}, {:.0}x{:.0})，显示区 {:.0}x{:.0}）",
        x, y, side, sx, sy, sw, sh, dw, dh
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

/// 弹窗矩形与选区矩形是否相交（同一坐标系，左上原点、y 向下）
fn overlaps(ax: f32, ay: f32, aw: f32, ah: f32, bx: f32, by: f32, bw: f32, bh: f32) -> bool {
    ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah
}

/// 弹窗摆放：优先选区下方；下方放不下就放上方；两边都放不下时往空间更大的一侧靠。
/// 关键是不能靠 clamp 把弹窗往上顶——那会让弹窗盖住选中的词。
///
/// 返回 (x, y, 说明, 高度上限, 是否锚底)。
/// GPUI 只能改窗口尺寸、不能移动窗口，所以：
///   放下方 → 窗口顶固定在选区下方，高度随内容向下长；
///   放上方 → 窗口顶固定在「选区上方 - 高度上限」，卡片贴窗口底部，内容向上长，绝不压选区。
fn popup_placement(
    dw: f32,
    dh: f32,
    sx: f32,
    sy: f32,
    sh: f32,
    want_h: f32,
) -> (f32, f32, &'static str, f32, bool) {
    let x = sx.clamp(
        SCREEN_MARGIN,
        (dw - POPUP_W - SCREEN_MARGIN).max(SCREEN_MARGIN),
    );
    let room_below = dh - SCREEN_MARGIN - (sy + sh + POPUP_GAP);
    let room_above = sy - POPUP_GAP - SCREEN_MARGIN;
    if room_below >= want_h {
        (
            x,
            sy + sh + POPUP_GAP,
            "下方",
            room_below.min(POPUP_MAX_H),
            false,
        )
    } else if room_above >= want_h {
        let h = room_above.min(POPUP_MAX_H);
        (x, sy - POPUP_GAP - h, "上方", h, true)
    } else if room_below >= room_above {
        (
            x,
            sy + sh + POPUP_GAP,
            "下方（空间不足）",
            room_below.min(POPUP_MAX_H),
            false,
        )
    } else {
        let h = room_above.min(POPUP_MAX_H);
        (x, sy - POPUP_GAP - h, "上方（空间不足）", h, true)
    }
}

/// 弹窗几何（逻辑点，屏幕左上原点）
#[derive(Clone, Copy, Debug)]
struct PopupGeom {
    x: f32,
    y: f32,
    /// 窗口允许的最大高度
    max_h: f32,
    /// 卡片贴窗口底部（放在选区上方时）
    anchor_bottom: bool,
    /// 显示器高度，用于换算 Cocoa 坐标
    dh: f32,
    /// 卡片当前高度（按内容量出来的）
    card_h: f32,
    /// 放上方还是下方（日志用）
    side: &'static str,
}

impl PopupGeom {
    /// 窗口被用户拖走之后，几何记录必须跟上：窗口位置变了，
    /// 卡片在里面的相对位置（anchor_bottom 等）没变，所以只有 x/y 跟着改。
    fn with_origin(&self, x: f32, y: f32) -> PopupGeom {
        PopupGeom { x, y, ..*self }
    }

    /// 窗口当前应有的高度
    fn win_h(&self) -> f32 {
        if self.anchor_bottom {
            self.max_h
        } else {
            self.card_h.min(self.max_h)
        }
    }

    /// 卡片矩形（Cocoa 坐标，左下原点），用于「点击弹窗外部关闭」。
    /// 用卡片而不是窗口：放上方时窗口里有透明空白区，点那里也算「弹窗外部」。
    fn card_cocoa(&self) -> (f64, f64, f64, f64) {
        let card_top = if self.anchor_bottom {
            self.y + self.win_h() - self.card_h
        } else {
            self.y
        };
        (
            self.x as f64,
            (self.dh - (card_top + self.card_h)) as f64,
            POPUP_W as f64,
            self.card_h as f64,
        )
    }
}

/// 一段文本在给定宽度、字号下的高度（pt）。
/// 注意 shape_text 是按段落返回 WrappedLine 的，折行数藏在它内部的 wrap_boundaries 里，
/// 所以必须用 WrappedLine::size(line_height)（= line_height ×（折行数+1））来量，
/// 直接数 lines.len() 只会得到段落数（单段落永远是 1）。
fn text_height(
    window: &Window,
    text: &str,
    font_size: f32,
    line_height: f32,
    width: f32,
) -> f32 {
    let style = window.text_style();
    let mut f = font(style.font_family.clone());
    f.weight = style.font_weight;
    f.style = style.font_style;
    let runs = [TextRun {
        len: text.len(),
        font: f,
        color: style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }];
    match window.text_system().shape_text(
        text.to_string().into(),
        px(font_size),
        &runs,
        Some(px(width)),
        None,
    ) {
        Ok(lines) => lines
            .iter()
            .map(|l| f32::from(l.size(px(line_height)).height))
            .sum::<f32>()
            .max(line_height),
        // 度量失败时按字符宽度粗略估：CJK 按 1em、其他按 0.5em 算
        Err(_) => {
            let em: f32 = text
                .chars()
                .map(|c| if (c as u32) > 0x2e80 { 1.0 } else { 0.5 })
                .sum();
            ((em * font_size) / width.max(1.)).ceil().max(1.) * line_height
        }
    }
}

/// 卡片应有的高度（pt）：内边距 + 拖动条 + 正文 + 页脚
fn card_height(window: &Window, view: &PopupView) -> f32 {
    let style = window.text_style();
    let rem = window.rem_size();
    let win_base = f32::from(style.font_size.to_pixels(rem));
    let lh = f32::from(style.line_height_in_pixels(rem));
    // 行高比沿用窗口默认样式的：正文自己定了字号（POPUP_FONT_PT），但行高比不变
    let unit = if win_base > 0. { lh / win_base } else { 1.2 };
    // 正文实际字号是 POPUP_FONT_PT（render 里的 text_size），量高度必须用同一个值
    let base = POPUP_FONT_PT;
    let text_w = POPUP_W - POPUP_PAD * 2.;
    // 单词模式：正文里的「读音：…」那一行会显示在标题里，量高度时要先把它拆出去，
    // 否则会多算一行
    let body = view.pron_and_body().1;
    let body_h = text_height(window, &body, base, base * unit, text_w);
    // 单词模式顶部多一行「词 + 读音」（text_lg = 1.125rem，下边距 8）
    let head_h = match view.word.as_deref() {
        // 标题里的词用 text_lg = 1.125rem，跟正文不是一个号，所以按 rem 基准量
        Some(w) => {
            let head_base = win_base * 1.125;
            text_height(window, w, head_base, head_base * unit, text_w) + 8.
        }
        None => 0.,
    };
    if std::env::var("BEEK_DEBUG").is_ok() {
        eprintln!(
            "[beek] 高度度量：正文 {:.0}pt（正文 {} 字符，宽 {:.0}，行高系数 {:.2}）",
            body_h,
            body.chars().count(),
            text_w,
            unit
        );
    }
    (POPUP_PAD + POPUP_DRAG_H + 6. + head_h + body_h + 12. + POPUP_FOOTER_H + POPUP_PAD).ceil() + 2.
}

/// 顶部拖动条（≡）在 Cocoa 坐标里可以被按住拖动的区域：
/// 卡片最上面「内边距 + 拖动条」那一条，整宽。
/// 做成纯函数是为了让自检能直接断言，不用真的动鼠标。
fn drag_handle_cocoa(geom: &PopupGeom) -> (f64, f64, f64, f64) {
    let (cx, cy, cw, ch) = geom.card_cocoa();
    let top = cy + ch;
    let h = (POPUP_PAD + POPUP_DRAG_H) as f64;
    (cx, top - h, cw, h)
}

/// 这次采样是否要「开始拖动弹窗」：上一刻没按、这一刻按下，且按在拖动条上。
/// 和 should_close_on_press 一样只在「空 → 按下」那一下成立，所以按住不放不会反复触发。
fn drag_starts(prev_buttons: i64, cur_buttons: i64, mx: f64, my: f64, handle: (f64, f64, f64, f64)) -> bool {
    if prev_buttons != 0 || cur_buttons == 0 {
        return false;
    }
    let (x, y, w, h) = handle;
    mx >= x && mx <= x + w && my >= y && my <= y + h
}

/// 是否应当因为「在弹窗外部按下鼠标」而关闭弹窗。
/// prev/cur 是前后两次采样的按键掩码：只有「空 → 按下」才算一次新点击，
/// 这样打开弹窗时那次还没松开的按下不会立刻把弹窗关掉。
/// rect 与 mx/my 都在 Cocoa 屏幕坐标系（左下原点、y 向上）。
fn should_close_on_press(
    prev_buttons: i64,
    cur_buttons: i64,
    mx: f64,
    my: f64,
    rect: (f64, f64, f64, f64),
) -> bool {
    if prev_buttons != 0 || cur_buttons == 0 {
        return false;
    }
    let (x, y, w, h) = rect;
    let inside = mx >= x && mx <= x + w && my >= y && my <= y + h;
    !inside
}

fn open_popup_window(
    cx: &mut AsyncApp,
    content: PopupContent,
    b: SelBounds,
    word: Option<String>,
) -> Option<(WindowHandle<PopupView>, PopupGeom)> {
    let (dw, dh) = display_size(cx);
    let (sx, sy, sw, sh) = gpui_rect(b, dh);
    // 起始高度只用来定位置；真实高度在拿到窗口后按内容量（同一轮事件内完成，不会闪）
    let (x, y, side, max_h, anchor_bottom) =
        popup_placement(dw, dh, sx, sy, sh, POPUP_MIN_USABLE);
    let win_h = if anchor_bottom { max_h } else { POPUP_MIN_H };
    let hit = overlaps(sx, sy, sw, sh, x, y, POPUP_W, win_h);
    eprintln!(
        "[beek] 弹窗位置：({:.0}, {:.0}) {}，选区=({:.0}, {:.0}, {:.0}x{:.0})，压住选区={}，高度上限={:.0}，锚底={}（显示区 {:.0}x{:.0}）",
        x, y, side, sx, sy, sw, sh, hit, max_h, anchor_bottom, dw, dh
    );
    let geom = PopupGeom {
        x,
        y,
        max_h,
        anchor_bottom,
        dh,
        card_h: win_h,
        side,
    };
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(x), px(y)),
                size: size(px(POPUP_W), px(win_h)),
            })),
            titlebar: None,
            kind: WindowKind::PopUp,
            focus: false,
            show: true,
            is_movable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            ..Default::default()
        },
        move |_window, cx| {
            cx.new(|_cx| PopupView {
                content,
                word,
                anchor_bottom,
                win_h,
                copied: false,
            })
        },
    )
    .ok()
    .map(|w| (w, geom))
}

/// 按当前内容重算卡片高度、必要时调整窗口尺寸，返回更新后的几何
fn sync_popup_geometry(
    cx: &mut AsyncApp,
    win: &WindowHandle<PopupView>,
    geom: PopupGeom,
) -> PopupGeom {
    let mut out = geom;
    if let Ok(g) = win.update(cx, |view, window, vcx| {
        let mut g = geom;
        g.card_h = card_height(window, view);
        let win_h = g.win_h();
        let cur_h = f32::from(window.bounds().size.height);
        if (win_h - cur_h).abs() > 0.5 {
            window.resize(size(px(POPUP_W), px(win_h)));
            eprintln!(
                "[beek] 弹窗高度：{:.0} → {:.0}（卡片 {:.0}，上限 {:.0}，{}）",
                cur_h, win_h, g.card_h, g.max_h, g.side
            );
        }
        view.win_h = win_h;
        vcx.notify();
        g
    }) {
        out = g;
    }
    out
}

/// 图标锚点。AX 直接给出的 range 矩形直接用；只有退化成「元素框」时才另做判断：
/// 元素框可能很大（整块文档区），把图标放在它的左上角同样会离选区很远。
/// 这时如果鼠标还在元素框里，说明用户刚用鼠标选的词 —— 用鼠标位置当锚点最接近真相。
/// 图标锚点：
/// - AX 给的矩形和「刚按下的鼠标点」位置一致（<120pt）→ 用 AX 的，更精确
/// - 差得很远 → 信鼠标。云之家这类应用会把选中文本挂在一个大容器上，
///   容器自己的矩形（或退化成元素框的左上角）离真正的选区几百像素，
///   而鼠标按下的那个点就是用户实际选中的地方（实测：选区在 y≈800，容器却在 y≈150）
/// - 没有鼠标动作（键盘选词）→ 用 AX 矩形；矩形退化时用元素框兜底
/// 把「某个点」变成图标锚点：以它为中心、宽=一个图标、高=一行，并整体上移一行。
/// 图标是按「右边缘对齐锚点 + 上方留 POPUP_GAP」摆放的，所以图标最终落在该点所在**那一行的上方**。
/// 若只上移 POPUP_GAP，而点位落在行中间，图标就会压住选中那行（Chrome 实测过）。
fn anchor_at(px: f64, py: f64) -> SelBounds {
    const LINE_PT: f64 = 18.;
    SelBounds {
        x: px - ICON_SIZE as f64 / 2.,
        y: py - LINE_PT,
        w: ICON_SIZE as f64,
        h: LINE_PT,
    }
}

/// 图标锚点。只有「元素自己给出的 range 矩形」完全可信；其余（元素框 / 拿不到矩形 / 窗口占位）
/// 都要靠鼠标来定位，按可靠性依次取：
///   1. 刚按下的点（最准；但双击的按下/抬起只隔几十毫秒，50ms 轮询可能漏掉）
///   2. **决定弹按钮这一刻的鼠标位置**（用户刚选完词，指针基本还在选区上；兜住上面漏掉的情况）
/// 都没有才退回 AX 给的那个不可信矩形（会落在元素左上角，也就是之前「图标飘到角落」的现象）。
fn anchor_for(
    bounds: &SelBounds,
    source: &str,
    press: Option<(Instant, (f64, f64))>,
    pointer: (f64, f64),
) -> (SelBounds, &'static str) {
    let trusted = matches!(source, "element" | "parent" | "tree" | "webarea");
    let far = |p: (f64, f64)| (p.0 - bounds.x).abs() >= 120. || (p.1 - bounds.y).abs() >= 120.;
    if let Some((at, p)) = press {
        if at.elapsed() < Duration::from_secs(3) && far(p) {
            return (anchor_at(p.0, p.1), "mouse");
        }
    }
    if !trusted && far(pointer) {
        return (anchor_at(pointer.0, pointer.1), "pointer");
    }
    (*bounds, if trusted { "ax" } else { "soft" })
}

/// 纯函数自检：元素框退化时的锚点选择。
/// 这是云之家那类「AX 很薄」的应用里图标飘走的根因所在，所以要能断言。
fn anchor_selftest() {
    let now = Instant::now();
    // 云之家真实数据：元素框 x=386 y=149（被截断成 40x20），用户实际在 (420,838) 划词
    let element_frame = SelBounds { x: 386., y: 149., w: 40., h: 20. };
    let range_rect = SelBounds { x: 231., y: 173., w: 33., h: 13. };
    let cases: [(&str, SelBounds, &str, Option<(Instant, (f64, f64))>, (f64, f64), &str); 5] = [
        // 云之家场景：元素框离鼠标 600+，信鼠标
        (
            "元素框 + 鼠标在别处（云之家）",
            element_frame,
            "element-frame",
            Some((now, (420., 838.))),
            (420., 838.),
            "mouse",
        ),
        // 键盘选词（很久没按鼠标）→ 用元素框兜底
        // 没抓到按下点（双击漏采样），但当前指针还在选区上 → 用指针
        (
            "元素框 + 只有当前指针",
            element_frame,
            "element-frame",
            None,
            (420., 838.),
            "pointer",
        ),
        // 正常应用：AX 矩形就在鼠标位置附近 → 用 AX 的（更精确）
        (
            "range 矩形 + 鼠标就在附近",
            range_rect,
            "element",
            Some((now, (240., 180.))),
            (240., 180.),
            "ax",
        ),
        // AX 矩形和刚按下的点差很远 → 判为可疑，信鼠标
        (
            "range 矩形 + 鼠标在别处",
            range_rect,
            "element",
            Some((now, (900., 900.))),
            (900., 900.),
            "mouse",
        ),
        // 没有鼠标信息（例如启动后第一次键盘选词）→ 用 AX 矩形
        ("range 矩形 + 无鼠标信息", range_rect, "element", None, (240., 180.), "ax"),
    ];
    let mut ok = true;
    for (name, b, source, press, pointer, want) in cases {
        let (_, got) = anchor_for(&b, source, press, pointer);
        if got != want {
            ok = false;
            eprintln!("[自检] 锚点判定失败：{} → {}（期望 {}）", name, got, want);
        }
    }
    eprintln!(
        "[自检] 图标锚点 {} 例：{}（ax=AX 矩形；mouse=刚按下的点；element-frame=元素框兜底）",
        cases.len(),
        if ok { "全部通过" } else { "有失败" }
    );
}

// ---------- 鼠标跟踪线程 ----------
//
// 为什么单独开一个线程：AX 查询一次可能要上百毫秒（要走树），如果在 AX 线程里顺带采样
// 鼠标，双击那种几十毫秒的按下/抬起就会掉进 AX 查询的空档里，永远采不到
// （「刚做过划词动作」这个前置条件因此不成立，剪贴板兜底和鼠标锚点全部失效）。
// NSEvent 读按键是纯内存操作，5ms 一次的开销可以忽略。
#[derive(Default)]
struct MouseTrack {
    /// 最近一次「按下」的时间与位置（屏幕左上原点）
    press: Option<(Instant, (f64, f64))>,
    /// 最近一次「松开」的时间：代表刚完成一次可能的划词动作
    release: Option<Instant>,
    /// 当前是否按着
    down: bool,
}

static MOUSE_TRACK: std::sync::OnceLock<std::sync::Mutex<MouseTrack>> = std::sync::OnceLock::new();

fn mouse_track() -> &'static std::sync::Mutex<MouseTrack> {
    MOUSE_TRACK.get_or_init(|| std::sync::Mutex::new(MouseTrack::default()))
}

fn spawn_mouse_tracker() {
    std::thread::spawn(|| {
        let mut prev: i64 = 0;
        loop {
            let (_, _, buttons) = mouse::state();
            if buttons != prev {
                let mut t = mouse_track().lock().unwrap();
                if prev == 0 && buttons != 0 {
                    t.press = Some((Instant::now(), mouse::position_topleft()));
                } else if prev != 0 && buttons == 0 {
                    t.release = Some(Instant::now());
                }
                t.down = buttons != 0;
                prev = buttons;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    });
}

/// 刚发生过一次「按下又松开」（2 秒内）→ 认为用户可能刚划完词
fn gesture_recent(track: &MouseTrack) -> bool {
    track
        .release
        .map(|at| at.elapsed() < Duration::from_secs(2))
        .unwrap_or(false)
        || track
            .press
            .map(|(at, _)| at.elapsed() < Duration::from_secs(2))
            .unwrap_or(false)
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
        // 鼠标状态全部来自独立的跟踪线程（见 spawn_mouse_tracker）
        let mut handled_release: Option<Instant> = None;
        loop {
            std::thread::sleep(Duration::from_millis(50));
            ticks += 1;
            let (last_press, gesture) = {
                let t = mouse_track().lock().unwrap();
                (t.press, gesture_recent(&t))
            };
            // 这一次「松开」还没判定过 → 给它一次取词机会（AX 拿不到时就走剪贴板兜底）
            #[allow(unused_mut)]
            let mut pending_selection_check = {
                let t = mouse_track().lock().unwrap();
                match t.release {
                    Some(at) if Some(at) != handled_release && at.elapsed() < Duration::from_millis(1500) => {
                        Some(at)
                    }
                    _ => None,
                }
            };
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
            // 文本和矩形必须来自同一个元素（见 ax::ax_selection 的注释），
            // 否则会出现「译文对、图标飘到别处」——云之家这类 AX 树很薄的应用上实测过
            let mut sel = unsafe { ax_selection(gesture) };
            // AX 什么都没给、而用户刚做了划词动作 → 这类应用（Safari / Muse / Grok Bot）
            // 不会把选区暴露给 AX，改用「模拟 ⌘C + 读剪贴板」兜底（可在菜单里关掉）。
            if sel.is_none() {
                if let Some(at) = pending_selection_check {
                    handled_release = Some(at);
                    {
                        let (px, py) = last_press.map(|(_, p)| p).unwrap_or((0., 0.));
                        let who = unsafe { ax::describe_frontmost() };
                        eprintln!(
                            "[beek] 取词：AX 没有返回选区（{}；鼠标刚在 ({:.0}, {:.0}) 划过）",
                            who, px, py
                        );
                        if config::load().clipboard_fallback {
                            match clipboard::copy_selection() {
                                Some(text) => {
                                    eprintln!(
                                        "[beek] 取词：剪贴板兜底拿到 {:?}",
                                        text.chars().take(20).collect::<String>()
                                    );
                                    sel = Some(ax::Selection {
                                        text,
                                        bounds: anchor_at(px, py),
                                        source: "clipboard",
                                    });
                                }
                                // 兜底跑了但没拿到内容：留日志，否则分不清是「没触发」还是「复制为空」
                                None => eprintln!(
                                    "[beek] 取词：剪贴板兜底也没拿到内容（该应用可能没响应 ⌘C）"
                                ),
                            }
                        }
                    }
                }
            } else {
                pending_selection_check = None;
            }
            let cur = sel.as_ref().map(|s| s.text.clone());
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
                            if let Some(s) = sel.as_ref() {
                                let (b, anchor) = anchor_for(
                                    &s.bounds,
                                    s.source,
                                    last_press,
                                    mouse::position_topleft(),
                                );
                                eprintln!(
                                    "[beek] 选区矩形（来源={}，锚点={}）：x={:.0} y={:.0} w={:.0} h={:.0}",
                                    s.source, anchor, b.x, b.y, b.w, b.h
                                );
                                let _ = tx.send(UiEvent::ShowIcon(b, last.clone()));
                                icon_shown = true;
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
    anchor_selftest();
    if std::env::var("BEEK_CLIP_SELFTEST").is_ok() {
        clipboard::selftest();
    }
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

    // ---- 单词模式：词典式详解（含读音）----
    eprintln!(
        "[自检] 单词判定：\"serendipity\"={}（期望 true）  \"hello world\"={}（期望 false）  \"智能体\"={}（期望 true）  超长词={}（期望 false）",
        translate::is_single_word("serendipity"),
        translate::is_single_word("hello world"),
        translate::is_single_word("智能体"),
        translate::is_single_word(&"a".repeat(40))
    );
    let (p1, b1) = translate::split_pron("读音：/ˌserənˈdɪpəti/\n- n. 意外发现珍宝的运气");
    eprintln!(
        "[自检] 读音拆分：读音={:?}（期望 Some(/ˌserənˈdɪpəti/)）正文={:?}",
        p1, b1
    );
    let (p2, _) = translate::split_pron("读音：/ˌserə");
    eprintln!("[自检] 读音未收完（还没有换行）不拆：{:?}（期望 None）", p2);
    let (p3, _) = translate::split_pron("这是一段译文，没有读音行");
    eprintln!("[自检] 没有读音行时不动正文：{:?}（期望 None）", p3);

    const WORD: &str = "serendipity";
    let t2 = Instant::now();
    match translate::explain(WORD) {
        Ok(s) => {
            let (pron, body) = translate::split_pron(&s);
            eprintln!(
                "[自检] 单词详解：总计 {}ms，读音={:?}，正文 {} 字",
                ms(t2),
                pron,
                body.chars().count()
            );
            eprintln!("[自检] 单词详解正文预览：\n{}", body);
        }
        Err(e) => eprintln!("[自检] 单词详解失败：{}", e),
    }

    let t3 = Instant::now();
    let mut first_word_chunk: Option<u128> = None;
    let streamed = translate::explain_stream(WORD, |chunk| {
        if first_word_chunk.is_none() {
            first_word_chunk = Some(ms(t3));
            eprintln!("[自检] 详解流式首字 {}ms：{:?}", ms(t3), chunk);
        }
    });
    eprintln!(
        "[自检] 详解流式：首字 {}ms，总计 {}ms，结果 {}",
        match first_word_chunk {
            Some(v) => format!("{}", v),
            None => "无内容".to_string(),
        },
        ms(t3),
        match streamed {
            Ok(s) => format!("{} 字", s.chars().count()),
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
        eprintln!("[自检] 注入模拟选区（屏幕坐标 600,400 220x20）→ 显示蜜蜂图标");
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
        // 版本比较是纯函数，顺带断言一遍（更新功能最容易错的就是这里）
        update::selftest();

        // ---- 菜单功能：设置 API Key → 检查更新 → 关于 ----
        // 配置写入这块会动真实 config.json，所以先快照、跑完恢复
        // （不用假 HOME：换掉 HOME 会让 AppKit 在某些阶段抛异常，自检跑不完）
        let cfg_path = config::config_path();
        let cfg_backup = std::fs::read(&cfg_path).ok();
        eprintln!(
            "[自检] 菜单功能：触发「设置 API Key…」（原配置已快照：{} 字节）",
            cfg_backup.as_ref().map(|b| b.len()).unwrap_or(0)
        );
        let _ = tx.send(UiEvent::OpenKeySettings);
        std::thread::sleep(Duration::from_millis(400));
        let saved = config::load().provider(config::ARK);
        eprintln!(
            "[自检] 配置回读：api_key 长度={}（期望 21）model={:?}（期望 selftest-model）配置路径={}",
            saved.api_key.chars().count(),
            saved.model,
            config::config_path().display()
        );

        eprintln!("[自检] 菜单功能：触发「检查更新…」（会真的请求 GitHub API）");
        let _ = tx.send(UiEvent::CheckUpdate);
        std::thread::sleep(Duration::from_secs(6));

        eprintln!("[自检] 菜单功能：触发「关于 BeeK」（自检模式走「检查更新」分支）");
        let _ = tx.send(UiEvent::OpenAbout);
        std::thread::sleep(Duration::from_secs(6));
        update::selftest_install();
        if let Ok(url) = std::env::var("BEEK_UPDATE_SELFTEST_URL") {
            update::selftest_download(&url);
        }
        // 恢复用户原配置（快照是什么就写回什么）
        match &cfg_backup {
            Some(bytes) => match std::fs::write(&cfg_path, bytes) {
                Ok(_) => eprintln!(
                    "[自检] 已恢复原配置（{} 字节）→ {}",
                    bytes.len(),
                    cfg_path.display()
                ),
                Err(e) => eprintln!("[自检] ⚠️ 恢复配置失败：{}", e),
            },
            None => {
                let _ = std::fs::remove_file(&cfg_path);
                eprintln!("[自检] 原本没有配置文件，已删除自检写入的 config.json");
            }
        }
        eprintln!("[自检] 菜单功能验证结束");

        // ---- 定位回归：弹窗不得盖住选区 ----
        // 用 dh=982 的屏幕算纯函数，覆盖「下方放得下 / 放不下要转上方」两种情形
        // （选区坐标是屏幕左上原点：500 = 屏幕中上部，900 = 贴近屏幕底边）
        for &(label, sel_y) in &[("屏幕中部", 500.0f64), ("屏幕下部", 900.0f64)] {
            let bb = SelBounds { x: 600.0, y: sel_y, w: 220.0, h: 20.0 };
            let (sx, sy, sw, sh) = gpui_rect(bb, 982.0);
            let (px_, py_, side, max_h, anchor) =
                popup_placement(1512.0, 982.0, sx, sy, sh, 200.);
            // 按最坏情况（卡片长到高度上限）检查是否压住选区
            let card_h = max_h;
            let card_top = if anchor { py_ + max_h - card_h } else { py_ };
            let hit = overlaps(sx, sy, sw, sh, px_, card_top, POPUP_W, card_h);
            eprintln!(
                "[自检] 定位({}): 选区 top={:.0} 高={:.0} → 弹窗 ({:.0}, {:.0}) {}，上限 {:.0}，锚底={}，压住选区={}（期望 false）",
                label, sy, sh, px_, py_, side, max_h, anchor, hit
            );
        }
        // 真实窗口：注入一个贴近屏幕底部的选区，看弹窗是否转到选区上方
        let bot = SelBounds { x: 600.0, y: 900.0, w: 220.0, h: 20.0 };
        eprintln!("[自检] 几何回归：注入贴近屏幕底部的选区（屏幕坐标 600,900 220x20）");
        let _ = tx.send(UiEvent::ShowIcon(bot, SAMPLE.to_string()));
        std::thread::sleep(Duration::from_millis(400));
        let _ = tx.send(UiEvent::StartTranslate(SAMPLE.to_string(), bot));
        std::thread::sleep(Duration::from_secs(1));
        menubar::dump_windows("自检：底部选区弹出时");
        let _ = tx.send(UiEvent::ClosePopup);
        std::thread::sleep(Duration::from_millis(300));

        // ---- 高度随内容增长 + 复制 ----
        // 用长文本：流式过程中应看到弹窗高度一段段变大，最后触发复制供外部校验
        let _ = tx.send(UiEvent::ShowIcon(b, LONG_SAMPLE.to_string()));
        std::thread::sleep(Duration::from_millis(300));
        let _ = tx.send(UiEvent::StartTranslate(LONG_SAMPLE.to_string(), b));
        std::thread::sleep(Duration::from_secs(4));
        menubar::dump_windows("自检：长文本流式结束后");
        // ---- 真实拖动：把弹窗挪到别处（用户拖拖动条时每 16ms 走的就是这个函数）----
        // 注意：AppKit 的窗口事务只能在主线程做，所以这里把动作发给主循环执行
        eprintln!("[自检] 真实拖动：请求主线程把弹窗左上角移到 Cocoa (520, 700)");
        let _ = tx.send(UiEvent::DragTest(520.0, 700.0));
        std::thread::sleep(Duration::from_millis(300));
        eprintln!("[自检] 触发复制（与点弹窗里的「复制」按钮同一条路径）");
        let _ = tx.send(UiEvent::CopyResult);
        std::thread::sleep(Duration::from_millis(600));
        let _ = tx.send(UiEvent::ClosePopup);
        std::thread::sleep(Duration::from_millis(300));

        // ---- 「点击弹窗外部关闭」判定（纯函数）----
        let r = (500.0f64, 300.0f64, 380.0f64, 320.0f64);
        eprintln!(
            "[自检] 外部点击判定：弹窗内新按下={}（期望 false）  弹窗内持续按住={}（期望 false）",
            should_close_on_press(0, 1, 600.0, 400.0, r),
            should_close_on_press(1, 1, 600.0, 400.0, r)
        );
        eprintln!(
            "[自检] 外部点击判定：弹窗外新按下={}（期望 true）   弹窗外持续按住={}（期望 false）",
            should_close_on_press(0, 1, 100.0, 100.0, r),
            should_close_on_press(1, 1, 100.0, 100.0, r)
        );
        eprintln!(
            "[自检] 外部点击判定：无按键={}（期望 false）        弹窗边界上(500,300)={}（期望 false）",
            should_close_on_press(0, 0, 100.0, 100.0, r),
            should_close_on_press(0, 1, 500.0, 300.0, r)
        );

        // ---- 拖动条（≡）判定（纯函数）----
        let g = PopupGeom {
            x: 600.,
            y: 590.,
            max_h: 320.,
            anchor_bottom: false,
            dh: 982.,
            card_h: 268.,
            side: "下方",
        };
        let h = drag_handle_cocoa(&g);
        let strip_h = (POPUP_PAD + POPUP_DRAG_H) as f64;
        eprintln!(
            "[自检] 拖动条区域(Cocoa)：({:.0}, {:.0}) {:.0}x{:.0}（期望上沿={:.0}、高={:.0}，卡片下沿={:.0}）",
            h.0, h.1, h.2, h.3, 982.0 - 590.0, strip_h, 982.0 - (590.0 + 268.0)
        );
        let (mx_, my_) = (h.0 + h.2 / 2.0, h.1 + h.3 / 2.0);
        eprintln!(
            "[自检] 拖动判定：条内新按下={}（期望 true）  条内持续按住={}（期望 false）",
            drag_starts(0, 1, mx_, my_, h),
            drag_starts(1, 1, mx_, my_, h)
        );
        eprintln!(
            "[自检] 拖动判定：条下方正文处按下={}（期望 false，不得靠拖正文移动窗口）  条上方按下={}（期望 false）",
            drag_starts(0, 1, mx_, h.1 - 5.0, h),
            drag_starts(0, 1, mx_, h.1 + h.3 + 5.0, h)
        );

        // ---- 单词模式：单个词应该出「词 + 读音 + 发音图标」标题行 ----
        let wb = SelBounds { x: 600.0, y: 240.0, w: 90.0, h: 20.0 };
        eprintln!("[自检] 单词模式：注入单个词（serendipity），期望弹窗顶部有「词 + 读音 + 线描发音图标」");
        let _ = tx.send(UiEvent::StartTranslate("serendipity".to_string(), wb));
        std::thread::sleep(Duration::from_secs(8));
        menubar::dump_windows("自检：单词模式弹窗（应比整句译文多一行标题）");
        eprintln!(
            "[自检] 单词模式判定：serendipity={}（期望 true）  hello world={}（期望 false）",
            translate::is_single_word("serendipity"),
            translate::is_single_word("hello world")
        );
        let (pron_demo, body_demo) = translate::split_pron("读音：/ˌserənˈdɪpəti/\n- n. 意外发现珍宝的运气");
        eprintln!(
            "[自检] 读音拆分：读音={:?} 正文={:?}",
            pron_demo, body_demo
        );
        let (not_done, _) = translate::split_pron("读音：/ˌserə");
        eprintln!("[自检] 读音行没收完（无换行）时不拆：{:?}（期望 None）", not_done);
        let _ = tx.send(UiEvent::ClosePopup);
        std::thread::sleep(Duration::from_millis(300));

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
            config::update(|c| c.show_in_dock = orig_show_in_dock);
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
    if std::env::var("BEEK_AX_PROBE").is_ok() {
        // 诊断模式：把一个应用的 AX 选区情况打出来，不开窗口
        unsafe { ax::ax_probe() };
        return;
    }
    if std::env::var("BEEK_CLIP_SELFTEST").is_ok() {
        // 只验证「模拟 ⌘C + 剪贴板」两个原语（先在前台应用里选好一段文字再跑）
        clipboard::selftest();
        return;
    }

    Application::new().run(|cx: &mut App| {
        let cfg = config::load();
        menubar::set_dock_visible(cfg.show_in_dock);
        // 事件通道先于菜单创建：菜单里的「设置 API Key / 检查更新 / 关于」
        // 都要把动作转回主循环执行
        let (tx, rx) = mpsc::channel::<UiEvent>();
        menubar::set_ui_sender(tx.clone());
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

        let selftest_mode = std::env::var("BEEK_SELFTEST").is_ok();
        if selftest_mode {
            spawn_selftest(tx.clone(), cfg.show_in_dock);
        }
        spawn_mouse_tracker();
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
            // 弹窗几何 + 上一次采样的鼠标按键掩码：用于「点击弹窗外部关闭」
            let mut popup_geom: Option<PopupGeom> = None;
            let mut prev_buttons: Option<i64> = None;
            // 正在拖动弹窗时记着「鼠标相对窗口左上角的偏移」（Cocoa 坐标）
            let mut drag: Option<(f64, f64)> = None;
            loop {
                while let Ok(ev) = rx.try_recv() {
                    match ev {
                        UiEvent::ShowIcon(b, text) => {
                            // 新选区：旧弹窗关闭，作废旧翻译
                            shown = None;
                            in_flight = None;
                            close_window(cx, &mut popup_win);
                            popup_geom = None;
                            if icon_win.is_none() {
                                icon_win = open_icon_window(cx, b, text, tx.clone());
                                // 记下窗口真实的 frame：算出来的位置和系统实际摆放的位置
                                // 可能不一致（比如被 AppKit 挪走），排查「图标飘走」时要能分辨
                                if let Some(w) = icon_win.as_ref() {
                                    if let Ok(actual) = w.update(cx, |_v, window, _cx| {
                                        let bo = window.bounds();
                                        (
                                            f32::from(bo.origin.x),
                                            f32::from(bo.origin.y),
                                            f32::from(bo.size.width),
                                            f32::from(bo.size.height),
                                        )
                                    }) {
                                        eprintln!(
                                            "[beek] 图标窗口实际 frame：({:.0}, {:.0}) {:.0}x{:.0}",
                                            actual.0, actual.1, actual.2, actual.3
                                        );
                                    }
                                }
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
                            popup_geom = None;
                            // 单个词 → 词典式详解（带读音）；一段话 → 只出译文
                            let word = if translate::is_single_word(&text) {
                                Some(text.clone())
                            } else {
                                None
                            };
                            if word.is_some() {
                                eprintln!("[beek] +{}ms 判定为单个词，走词典详解：{:?}", ms(t0), text);
                            }
                            // 关键：先开弹窗再等网络，用户点完立刻就有东西出来
                            let opened =
                                open_popup_window(cx, PopupContent::Loading, b, word.clone());
                            if let Some((w, geom)) = opened {
                                // 立刻按内容量一次高度（此时正文是「翻译中…」）
                                popup_geom = Some(sync_popup_geometry(cx, &w, geom));
                                popup_win = Some(w);
                            }
                            // 忽略「开弹窗那一刻还按着的鼠标」，否则点图标的那一下会立刻把它关掉
                            prev_buttons = None;
                            eprintln!("[beek] +{}ms 点击图标 → 弹窗立即显示（id={}）", ms(t0), id);
                            let tx2 = tx.clone();
                            std::thread::spawn(move || {
                                let started = Instant::now();
                                let mut first_at: Option<u128> = None;
                                let mut on_chunk = |chunk: &str| {
                                    if first_at.is_none() {
                                        first_at = Some(started.elapsed().as_millis());
                                    }
                                    let _ = tx2.send(UiEvent::TranslateChunk(id, chunk.to_string()));
                                };
                                let result = if word.is_some() {
                                    translate::explain_stream(&text, &mut on_chunk)
                                } else {
                                    translate::translate_stream(&text, &mut on_chunk)
                                };
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
                                    // 内容变长了 → 重算高度（向下长 / 向上长）
                                    if let Some(g) = popup_geom {
                                        popup_geom = Some(sync_popup_geometry(cx, w, g));
                                    }
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
                                if let Some(g) = popup_geom {
                                    if let Some(w) = popup_win.as_ref() {
                                        popup_geom = Some(sync_popup_geometry(cx, w, g));
                                    }
                                }
                                // 自检模式：结果一落地立刻走一次复制，避免等待期间被外部点击打断
                                if selftest_mode {
                                    if let Some(w) = popup_win.as_ref() {
                                        let _ = w.update(cx, |view, _window, vcx| view.copy(vcx));
                                        eprintln!(
                                            "[beek] +{}ms 自检：译文落地即复制到剪贴板（同复制按钮走的函数）",
                                            ms(t0)
                                        );
                                    }
                                }
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
                        UiEvent::CopyResult => {
                            // 自检：走 PopupView 的真实复制逻辑（和点「复制」按钮同一条路径）
                            if let Some(w) = &popup_win {
                                let ok = w
                                    .update(cx, |view, _window, vcx| view.copy(vcx))
                                    .is_ok();
                                eprintln!("[beek] +{}ms 复制译文到剪贴板（ok={}）", ms(t0), ok);
                            } else {
                                eprintln!("[beek] +{}ms 复制请求：弹窗已不在，跳过", ms(t0));
                            }
                        }
                        UiEvent::DragTest(x, y_top) => {
                            // 自检：主线程上走一次真实的窗口移动（拖动条拖动用的就是它）
                            let ok = menubar::popup_window_move(x, y_top);
                            eprintln!(
                                "[beek] +{}ms 自检：主线程移动弹窗到 Cocoa ({:.0}, {:.0}) → {}（期望 true）",
                                ms(t0),
                                x,
                                y_top,
                                ok
                            );
                            menubar::dump_windows("自检：真实拖动之后（期望 x=520、frame 上沿=700）");
                        }
                        UiEvent::OpenKeySettings => {
                            // AppKit 对话框只能在主线程弹，这里就是主线程
                            let cur = config::load().provider(config::ARK);
                            let cur_model = cur.model.clone().unwrap_or_default();
                            match dialogs::key_settings_dialog(cur.api_key.clone(), cur_model) {
                                Some(input) => {
                                    let key = input.api_key.clone();
                                    let model = input.model.clone();
                                    config::update(|c| {
                                        c.set_provider(
                                            config::ARK,
                                            config::ProviderConfig {
                                                api_key: key.clone(),
                                                model: if model.is_empty() { None } else { Some(model.clone()) },
                                                base_url: None,
                                            },
                                        );
                                    });
                                    eprintln!(
                                        "[beek] +{}ms 已保存 API Key（长度 {}，模型 {}）到 {}",
                                        ms(t0),
                                        key.chars().count(),
                                        if model.is_empty() { "默认".to_string() } else { model.clone() },
                                        config::config_path().display()
                                    );
                                    if key.is_empty() {
                                        dialogs::alert(
                                            "已清空 API Key",
                                            "配置里没有 Key 了。取词翻译将回落到环境变量 BEEK_ARK_KEY；\
                                             两者都没有时会提示「未配置 API Key」。",
                                            &["好"],
                                        );
                                    } else {
                                        dialogs::alert(
                                            "已保存",
                                            "翻译会立即使用新的 Key，不需要重启。",
                                            &["好"],
                                        );
                                    }
                                }
                                None => eprintln!("[beek] +{}ms 设置 API Key：用户取消", ms(t0)),
                            }
                        }
                        UiEvent::OpenAbout => {
                            match dialogs::about_dialog() {
                                0 => {
                                    let _ = tx.send(UiEvent::CheckUpdate);
                                }
                                1 => update::open_url(update::README_URL),
                                _ => {}
                            }
                        }
                        UiEvent::CheckUpdate => {
                            eprintln!(
                                "[beek] +{}ms 检查更新（当前 v{}，仓库 {}）",
                                ms(t0),
                                update::CURRENT_VERSION,
                                update::REPO
                            );
                            let tx2 = tx.clone();
                            std::thread::spawn(move || {
                                let r = update::check();
                                match &r {
                                    Ok(Some(rel)) => eprintln!(
                                        "[beek] 检查更新：远端 {}（资产 {}）",
                                        rel.tag,
                                        rel.asset_url.as_deref().unwrap_or("无")
                                    ),
                                    Ok(None) => eprintln!("[beek] 检查更新：远端还没有任何 release"),
                                    Err(e) => eprintln!("[beek] 检查更新失败：{}", e),
                                }
                                let _ = tx2.send(UiEvent::UpdateChecked(r));
                            });
                        }
                        UiEvent::UpdateChecked(result) => match result {
                            Err(e) => {
                                if dialogs::alert(
                                    "检查更新失败",
                                    &format!("{}\n\n当前版本 v{}。", e, update::CURRENT_VERSION),
                                    &["打开 Release 页面", "好"],
                                ) == 0
                                {
                                    update::open_url(update::RELEASES_URL);
                                }
                            }
                            Ok(None) => {
                                if dialogs::alert(
                                    "暂时没有可用的新版本",
                                    &format!(
                                        "仓库 {} 还没有发布过 release。\n\n当前版本 v{}。\
                                         发布页：{}",
                                        update::REPO,
                                        update::CURRENT_VERSION,
                                        update::RELEASES_URL
                                    ),
                                    &["打开 Release 页面", "好"],
                                ) == 0
                                {
                                    update::open_url(update::RELEASES_URL);
                                }
                            }
                            Ok(Some(rel)) => {
                                if !update::is_newer(&rel.version, update::CURRENT_VERSION) {
                                    dialogs::alert(
                                        "已是最新版本",
                                        &format!(
                                            "当前 v{}，远端最新 {}。",
                                            update::CURRENT_VERSION, rel.tag
                                        ),
                                        &["好"],
                                    );
                                } else {
                                    let notes: String = rel
                                        .notes
                                        .lines()
                                        .take(12)
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    let can_auto = rel.asset_url.is_some();
                                    let body = format!(
                                        "当前 v{} → 新版本 {}\n\n{}{}",
                                        update::CURRENT_VERSION,
                                        rel.tag,
                                        if notes.trim().is_empty() {
                                            "（这条 release 没写说明）\n".to_string()
                                        } else {
                                            format!("{}\n", notes.trim())
                                        },
                                        if can_auto {
                                            ""
                                        } else {
                                            "\n注意：这条 release 没有附带升级包，只能手动下载。"
                                        }
                                    );
                                    let choice = if can_auto {
                                        dialogs::alert(
                                            &format!("发现新版本 {}", rel.tag),
                                            &body,
                                            &["下载并更新", "打开 Release 页面", "稍后"],
                                        )
                                    } else {
                                        dialogs::alert(
                                            &format!("发现新版本 {}", rel.tag),
                                            &body,
                                            &["打开 Release 页面", "稍后"],
                                        ) + 1
                                    };
                                    match choice {
                                        // 下载并更新 → 后台下载 + 解压，回来再确认一次
                                        0 => {
                                            eprintln!(
                                                "[beek] +{}ms 开始下载升级包 {}",
                                                ms(t0),
                                                update::asset_name(&rel.version)
                                            );
                                            let tx2 = tx.clone();
                                            let rel2 = rel.clone();
                                            std::thread::spawn(move || {
                                                match update::prepare(&rel2) {
                                                    Ok(app) => {
                                                        let _ = tx2.send(UiEvent::UpdatePrepared(
                                                            app,
                                                            rel2.version.clone(),
                                                        ));
                                                    }
                                                    Err(e) => {
                                                        let _ =
                                                            tx2.send(UiEvent::UpdateFailed(e));
                                                    }
                                                }
                                            });
                                        }
                                        1 => update::open_url(&rel.page_url),
                                        _ => eprintln!("[beek] +{}ms 用户选择稍后更新", ms(t0)),
                                    }
                                }
                            }
                        },
                        UiEvent::UpdatePrepared(new_app, version) => {
                            eprintln!(
                                "[beek] +{}ms 升级包已就绪：{} → v{}",
                                ms(t0),
                                new_app.display(),
                                version
                            );
                            let script = update::install_script(&new_app, &version);
                            match script {
                                Err(e) => {
                                    dialogs::alert("更新失败", &e, &["好"]);
                                }
                                Ok(path) => {
                                    let go = dialogs::alert(
                                        &format!("已下载 v{}，现在更新？", version),
                                        "点「立即更新」后 BeeK 会退出，替换 /Applications/BeeK.app \
                                         并在几秒后自动启动新版本。",
                                        &["立即更新", "稍后"],
                                    ) == 0;
                                    if go {
                                        eprintln!(
                                            "[beek] +{}ms 执行替换脚本 {}，准备退出",
                                            ms(t0),
                                            path.display()
                                        );
                                        if let Err(e) = update::run_script(&path) {
                                            dialogs::alert("更新失败", &e, &["好"]);
                                        } else {
                                            // 脚本会等这个进程退出后再替换 bundle
                                            std::process::exit(0);
                                        }
                                    } else {
                                        eprintln!(
                                            "[beek] +{}ms 用户选择稍后替换（升级包留在 {}）",
                                            ms(t0),
                                            new_app.display()
                                        );
                                    }
                                }
                            }
                        }
                        UiEvent::UpdateFailed(e) => {
                            eprintln!("[beek] +{}ms 更新失败：{}", ms(t0), e);
                            if dialogs::alert(
                                "更新失败",
                                &format!("{}\n\n可以从 Release 页面手动下载安装。", e),
                                &["打开 Release 页面", "好"],
                            ) == 0
                            {
                                update::open_url(update::RELEASES_URL);
                            }
                        }
                        UiEvent::ClosePopup => {
                            let was = shown.take();
                            // 作废在飞行中的翻译：关闭之后不再被迟到的结果弹回来
                            in_flight = None;
                            close_window(cx, &mut popup_win);
                            popup_geom = None;
                            eprintln!(
                                "[beek] +{}ms 关闭弹窗（id={:?}，已流式写入 {} 段）",
                                ms(t0),
                                was,
                                chunks_applied
                            );
                        }
                    }
                }
                // 「点击弹窗外部关闭」+「按住拖动条拖动弹窗」：弹窗是 focus:false 的
                // 非激活窗口，点它外面/拖它都不会产生可用的窗口事件（gpui 的 mac 平台
                // 也没有拖动窗口的 API），所以两件事都靠主动轮询全局鼠标状态来做。
                if let Some(g) = popup_geom {
                    let rect = g.card_cocoa();
                    let handle = drag_handle_cocoa(&g);
                    let (mx, my, buttons) = mouse::state();
                    match prev_buttons {
                        // 第一次采样只记状态，不判定
                        None => prev_buttons = Some(buttons),
                        Some(prev) => {
                            if let Some((off_x, off_y)) = drag {
                                if buttons == 0 {
                                    drag = None;
                                    eprintln!(
                                        "[beek] +{}ms 拖动结束：弹窗左上角 ({:.0}, {:.0})",
                                        ms(t0),
                                        g.x,
                                        g.y
                                    );
                                } else {
                                    // 窗口左上角 = 鼠标位置 - 按下时的偏移
                                    let nx = mx - off_x;
                                    let ntop = my - off_y;
                                    if menubar::popup_window_move(nx, ntop) {
                                        // 窗口已经被 AppKit 挪走了，几何记录必须跟上，
                                        // 否则「弹窗外部」的判定矩形还留在原地
                                        popup_geom = Some(
                                            g.with_origin(nx as f32, (g.dh as f64 - ntop) as f32),
                                        );
                                    }
                                }
                            } else if drag_starts(prev, buttons, mx, my, handle) {
                                // 记下「鼠标相对窗口左上角」的偏移，拖动时才不会跳
                                let (dx, dy) = (
                                    mx - g.x as f64,
                                    my - (g.dh as f64 - g.y as f64),
                                );
                                drag = Some((dx, dy));
                                eprintln!(
                                    "[beek] +{}ms 按住拖动条 → 开始拖动弹窗（鼠标 {:.0},{:.0}，窗口 ({:.0},{:.0})）",
                                    ms(t0),
                                    mx,
                                    my,
                                    g.x,
                                    g.y
                                );
                            } else if should_close_on_press(prev, buttons, mx, my, rect) {
                                eprintln!(
                                    "[beek] +{}ms 在弹窗外部按下鼠标（{:.0},{:.0}，弹窗 ({:.0},{:.0},{:.0}x{:.0})）→ 关闭",
                                    ms(t0),
                                    mx,
                                    my,
                                    rect.0,
                                    rect.1,
                                    rect.2,
                                    rect.3
                                );
                                let _ = tx.send(UiEvent::ClosePopup);
                            }
                            prev_buttons = Some(buttons);
                        }
                    }
                } else {
                    prev_buttons = None;
                    drag = None;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
            }
        })
        .detach();
    });
}
