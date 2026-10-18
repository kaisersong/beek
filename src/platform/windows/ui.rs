use super::{
    input::{self, ClipboardIntent, TextInput},
    native::{self, Event},
    selection::{Coordinator, Outcome},
    store::Store,
    util,
};
use crate::{
    core::{
        appearance::{POPUP_DRAG_H, POPUP_FONT_PT, POPUP_MAX_H, POPUP_PAD, POPUP_W},
        policy::TTL_MS,
        protocol::{Rect, Selection, Source, Status},
        settings::{KeyEdit, Settings},
        stream::{self, Mailbox, Pool, Progress},
    },
    icons::{self, IconKind},
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
    hovered: bool,
}
impl Render for Bee {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let image = if self.loading {
            IconKind::Disabled
        } else if self.hovered {
            IconKind::Hover
        } else {
            IconKind::Default
        }
        .render_image(cx);
        div()
            .id("selection-bee")
            .size(px(icons::SELECTION_BUTTON_PT))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .child(
                img(image)
                    .size(px(icons::SELECTION_BUTTON_PT * icons::SELECTION_CROP_ZOOM))
                    .flex_shrink_0(),
            )
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.hovered = *hovered;
                cx.notify();
            }))
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
struct Panel {
    active: bool,
    focus: FocusHandle,
    selection: Selection,
    body: String,
    status: String,
    error: bool,
    copied_until: Option<Instant>,
    anchor: Option<Rect>,
    layout_height: f32,
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
    fn word(&self) -> Option<&str> {
        crate::core::prompts::is_single_word(&self.selection.text)
            .then(|| self.selection.text.trim())
    }
    fn copy_text(&self) -> String {
        let (pron, body) = stream::split_pron(&self.body);
        match (self.word(), pron) {
            (Some(word), Some(pron)) => format!("{word}  {pron}\n{body}"),
            (Some(word), None) => format!("{word}\n{body}"),
            _ => body,
        }
    }
    fn copy(&mut self, _: &CopyPreview, _: &mut Window, _: &mut Context<Self>) {
        self.copy_result();
    }
    fn copy_result(&self) {
        if !self.body.is_empty() && !self.error {
            let _ = self.tx.try_send(Command::Copy(self.copy_text(), unsafe {
                windows::Win32::System::DataExchange::GetClipboardSequenceNumber()
            }));
        }
    }
    fn close(&mut self, _: &CloseView, _: &mut Window, _: &mut Context<Self>) {
        let _ = self.tx.try_send(Command::Close);
    }
}

// Use the renderer's shaped, wrapped lines, so CJK and long paragraphs size the
// window using the same font metrics as the visible content (as on macOS).
fn text_height(window: &Window, text: &str, font_size: f32, width: f32) -> f32 {
    let style = window.text_style();
    let rem = window.rem_size();
    let base = f32::from(style.font_size.to_pixels(rem));
    let unit = if base > 0. {
        f32::from(style.line_height_in_pixels(rem)) / base
    } else {
        1.2
    };
    let line_height = font_size * unit;
    let mut font = font(style.font_family.clone());
    font.weight = style.font_weight;
    font.style = style.font_style;
    let runs = [TextRun {
        len: text.len(),
        font,
        color: style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }];
    match window.text_system().shape_text(
        text.to_owned().into(),
        px(font_size),
        &runs,
        Some(px(width)),
        None,
    ) {
        Ok(lines) => lines
            .iter()
            .map(|line| f32::from(line.size(px(line_height)).height))
            .sum::<f32>()
            .max(line_height),
        Err(_) => {
            let em: f32 = text
                .chars()
                .map(|c| if c as u32 > 0x2e80 { 1. } else { 0.5 })
                .sum();
            (em * font_size / width.max(1.)).ceil().max(1.) * line_height
        }
    }
}
impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview = self.snapshot && !self.active && self.body.is_empty() && !self.error;
        let (pron, body) = stream::split_pron(&self.body);
        let word = self.word().map(str::to_owned);
        let shown = if preview {
            self.selection.text.clone()
        } else if self.error {
            format!("翻译失败：{}", self.status)
        } else if self.body.is_empty() {
            if self.active && self.status != "正在翻译…" {
                self.status.clone()
            } else if word.is_some() {
                "查询中…".into()
            } else {
                "翻译中…".into()
            }
        } else {
            body
        };
        let footer = if self.status.starts_with("复制未完成") {
            self.status.clone()
        } else if preview {
            self.status.clone()
        } else {
            "点击弹窗外任意处关闭".into()
        };
        let content_width = POPUP_W - POPUP_PAD * 2.;
        let header_height = if word.is_some() && !preview {
            let font_size = f32::from(window.rem_size()) * 1.125;
            text_height(window, word.as_deref().unwrap(), font_size, content_width) + 8.
        } else {
            0.
        };
        let footer_height = text_height(
            window,
            &footer,
            12.,
            content_width - if preview { 92. } else { 28. },
        )
        .max(22.);
        let height = (POPUP_PAD * 2.
            + POPUP_DRAG_H
            + 6.
            + header_height
            + text_height(window, &shown, POPUP_FONT_PT, content_width)
            + 12.
            + footer_height
            + 2.)
            .ceil()
            .min(POPUP_MAX_H);
        if !self.snapshot && (self.layout_height - height).abs() > 0.5 {
            self.layout_height = height;
            cx.defer_in(window, move |panel, window, cx| {
                let _ = native::resize_popup(window, panel.anchor, [POPUP_W as f64, height as f64]);
                cx.notify();
            });
        }
        let copied = self
            .copied_until
            .is_some_and(|until| until > Instant::now());
        #[cfg(feature = "test-fixtures")]
        super::diagnostics::event(
            "panel_layout",
            serde_json::json!({"width":f32::from(window.viewport_size().width),"height":f32::from(window.viewport_size().height)}),
        );
        div()
            .id("translation-card")
            .w_full()
            .h(window.viewport_size().height)
            .p(px(POPUP_PAD))
            .rounded(px(12.))
            .bg(white())
            .shadow_lg()
            .flex()
            .flex_col()
            .key_context("BeeKPreview")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::translate))
            .on_action(cx.listener(Self::close))
            .on_action(cx.listener(Self::copy))
            .child(
                div()
                    .id("drag-header")
                    .w_full()
                    .h(px(POPUP_DRAG_H))
                    .mb(px(6.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor(gpui::CursorStyle::ClosedHand)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().w(px(22.)).h(px(2.)).rounded(px(1.)).bg(rgb(0xcccccc)))
                            .child(div().w(px(22.)).h(px(2.)).rounded(px(1.)).bg(rgb(0xcccccc))),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, _| {
                            if this.snapshot {
                                window.start_window_move();
                            } else {
                                this.anchor = None;
                                let _ = this.tx.try_send(Command::Drag(util::cursor()));
                            }
                        }),
                    ),
            )
            .child(
                div()
                    .id("translation-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .when(!preview, |d| {
                        d.children(word.clone().map(|word| {
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_1()
                                .mb(px(8.))
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(0x1f1f1f))
                                        .child(word),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(if pron.is_some() {
                                            0x666666
                                        } else {
                                            0xcccccc
                                        }))
                                        .child(pron.clone().unwrap_or_else(|| "读音…".into())),
                                )
                                .child(
                                    div()
                                        .id("speak")
                                        .size(px(icons::SPEAK_ICON_PT + 6.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(5.))
                                        .hover(|s| s.bg(rgb(0xe8e8e8)))
                                        .cursor_pointer()
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, _| {
                                                let _ = this.tx.try_send(Command::Speak(
                                                    this.selection.text.clone(),
                                                ));
                                            }),
                                        )
                                        .child(
                                            img(icons::speak_outline(cx))
                                                .size(px(icons::SPEAK_ICON_PT)),
                                        ),
                                )
                        }))
                    })
                    .child(
                        div()
                            .text_size(px(POPUP_FONT_PT))
                            .text_color(rgb(if self.error {
                                0xcc3333
                            } else if self.body.is_empty() && !preview {
                                0x888888
                            } else {
                                0x1f1f1f
                            }))
                            .child(shown),
                    ),
            )
            .child(
                div()
                    .mt(px(12.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(div().text_xs().text_color(rgb(0xbbbbbb)).child(footer))
                    .when(preview, |d| {
                        d.child(
                            div()
                                .id("translate")
                                .flex_shrink_0()
                                .px_2()
                                .py_1()
                                .rounded(px(5.))
                                .text_size(px(12.))
                                .text_color(rgb(0x666666))
                                .hover(|s| s.bg(rgb(0xe8e8e8)))
                                .cursor_pointer()
                                .child("翻译 (Enter)")
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, window, cx| {
                                        this.translate(&TranslateSnapshot, window, cx);
                                    }),
                                ),
                        )
                    })
                    .when(!preview, |d| {
                        d.child(
                            div()
                                .id("copy")
                                .size(px(20.))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(5.))
                                .hover(|s| s.bg(rgb(0xe8e8e8)))
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, _| this.copy_result()),
                                )
                                .child(
                                    div()
                                        .relative()
                                        .size(px(12.))
                                        .child(
                                            div()
                                                .absolute()
                                                .left(px(4.))
                                                .top(px(0.))
                                                .size(px(8.))
                                                .border_1()
                                                .border_color(rgb(0xbbbbbb))
                                                .rounded(px(2.)),
                                        )
                                        .child(
                                            div()
                                                .absolute()
                                                .left(px(0.))
                                                .top(px(4.))
                                                .size(px(8.))
                                                .bg(white())
                                                .border_1()
                                                .border_color(rgb(if copied {
                                                    0x2f9e44
                                                } else {
                                                    0x888888
                                                }))
                                                .rounded(px(2.)),
                                        ),
                                ),
                        )
                    }),
            )
    }
}
struct Form {
    _subscriptions: Vec<Subscription>,
    save_revision: Option<(Vec<u64>, [bool; 5])>,
    fields: Vec<Entity<TextInput>>,
    cfg: Settings,
    status: String,
    busy: bool,
    clear: bool,
    tx: Tx,
}
impl Form {
    fn signature(&self, cx: &App) -> (Vec<u64>, [bool; 5]) {
        (
            self.fields.iter().map(|f| f.read(cx).revision()).collect(),
            [
                self.cfg.selection_enabled,
                self.cfg.copy_to_popup,
                self.cfg.hotkey_enabled,
                self.cfg.autostart,
                self.clear,
            ],
        )
    }
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
            self.save_revision = Some(self.signature(cx));
            self.status = "正在保存…".into();
            cx.notify();
        }
    }
}
impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let labels = [
            if self.cfg.provider.credential_ref.is_some() {
                "API Key（已保存；输入新 Key 可替换）"
            } else {
                "API Key（输入后显示为星号）"
            },
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
fn open_form(cx: &mut App, tx: Tx, cfg: Settings, status: String) -> Option<WindowHandle<Form>> {
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
                            f.set_saved_secret(cfg.provider.credential_ref.is_some(), cx);
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
                                |this: &mut Form, field, event: &ClipboardIntent, cx| {
                                    if this
                                        .tx
                                        .try_send(Command::Edit(field, event.clone()))
                                        .is_err()
                                    {
                                        this.status = "操作队列繁忙，请稍后重试".into();
                                        cx.notify();
                                    }
                                },
                            )
                        })
                        .collect();
                    Form {
                        _subscriptions: subscriptions,
                        save_revision: None,
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
        .map_err(|_| {
            super::diagnostics::event("window_open_failed", serde_json::json!({"kind":"settings"}));
            util::fatal("无法创建设置窗口，请检查显卡驱动后重试");
        })
        .ok()?;
    let _ = handle.update(cx, |_, w, _| {
        let _ = native::keep_accessible(w);
        native::activate(w)
    });
    Some(handle)
}
fn open_bee(cx: &mut App, tx: Tx, s: Selection) -> Option<WindowHandle<Bee>> {
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
                window_min_size: Some(size(px(1.), px(1.))),
                window_background: WindowBackgroundAppearance::Transparent,
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(
                        px(icons::SELECTION_BUTTON_PT),
                        px(icons::SELECTION_BUTTON_PT),
                    ),
                ))),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| Bee {
                    selection: s,
                    tx,
                    loading: false,
                    hovered: false,
                })
            },
        )
        .map_err(|_| {
            super::diagnostics::event(
                "window_open_failed",
                serde_json::json!({"kind":"selection"}),
            )
        })
        .ok()?;
    // Let GPUI finish its queued initial resize before setting popup geometry
    // and z-order; its Windows resize otherwise resets both after we show it.
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(16))
            .await;
        let _ = w.update(cx, |_, window, cx| {
            let _ = native::show_selection(window, anchor, icons::SELECTION_BUTTON_PT as f64);
            cx.notify();
        });
    })
    .detach();
    Some(w)
}
fn open_panel(cx: &mut App, tx: Tx, s: Selection, snapshot: bool) -> Option<WindowHandle<Panel>> {
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
                    Bounds::centered(None, size(px(POPUP_W), px(160.)), cx)
                } else {
                    Bounds::new(point(px(0.), px(0.)), size(px(POPUP_W), px(160.)))
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
                        error: false,
                        copied_until: None,
                        anchor: Some(anchor),
                        layout_height: 0.,
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
        .map_err(|_| {
            super::diagnostics::event(
                "window_open_failed",
                serde_json::json!({"kind":"translation"}),
            )
        })
        .ok()?;
    if snapshot {
        let _ = w.update(cx, |_, window, _| {
            let _ = native::keep_accessible(window);
            native::activate(window)
        });
    } else {
        cx.spawn(async move |cx| {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            let _ = w.update(cx, |panel, window, cx| {
                let _ = native::show_popup(
                    window,
                    anchor,
                    [
                        POPUP_W as f64,
                        if panel.layout_height > 0. {
                            panel.layout_height as f64
                        } else {
                            112.
                        },
                    ],
                );
                cx.notify();
            });
        })
        .detach();
    }
    Some(w)
}
/// Visual QA uses the real views and native popup geometry, without opening the
/// settings store, installing hooks, reading a selection or making a request.
#[cfg(feature = "test-fixtures")]
pub fn preview(state: &str) {
    let state = state.to_owned();
    Application::new().run(move |cx| {
        let (tx, _rx) = mpsc::sync_channel(32);
        let selection = Selection {
            source: Source { hwnd: 0, pid: 1, created: 1, generation: 1 },
            epoch: 1,
            runtime_id: vec![],
            text: if state == "paragraph" || state == "long" {
                "Selection translation should feel instant and unobtrusive.".into()
            } else { "serendipity".into() },
            rects: vec![Rect { x: 400., y: if state == "edge" { 990. } else { 300. }, width: 180., height: 22. }],
            clipboard_sequence: None,
        };
        if state == "selection" {
            let _ = open_bee(cx, tx, selection);
        } else if let Some(handle) = open_panel(cx, tx, selection, state == "snapshot") {
            let _ = handle.update(cx, |panel, _, cx| {
                panel.body = match state.as_str() {
                    "loading" | "error" | "snapshot" => String::new(),
                    "paragraph" => "划词翻译应当响应迅速，不打扰阅读。".into(),
                    "long" => (1..=30).map(|i| format!("第{i}行：这是公开的长文本测试，验证卡片高度上限与内部滚动。\n")).collect(),
                    _ => "读音：/ˌserənˈdɪpəti/\n- n. 意外发现美好事物的运气\n- 例句：It was pure serendipity. 这纯属意外之喜。\n- 常见搭配：a moment of serendipity".into(),
                };
                panel.error = state == "error";
                panel.active = state == "loading";
                if panel.error { panel.status = "网络连接暂时不可用，请重试".into(); }
                if state == "copied" { panel.copied_until = Some(Instant::now() + Duration::from_secs(90)); }
                cx.notify();
            });
        }
        cx.on_window_closed(|cx| { if cx.windows().is_empty() { cx.quit(); } }).detach();
        cx.spawn(async |cx| {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            let _ = cx.update(|cx| cx.refresh_windows());
            cx.background_executor().timer(Duration::from_secs(90)).await;
            let _ = cx.update(|cx| cx.quit());
        }).detach();
    });
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
fn selection_status(status: Status) -> &'static str {
    match status {
        Status::TooLong => "选区或剪贴板文本超过 16384 字符，请缩小范围",
        Status::Protected => "该输入区域受保护，未读取文本",
        Status::Empty => "没有可用的文本选区或纯文本剪贴板内容",
        Status::SourceChanged => "来源或剪贴板已变化，请重新选择或重试",
        Status::Busy => "选区检测繁忙，请稍后重试",
        _ => "当前应用未提供可用选区，请使用托盘的剪贴板预览",
    }
}
fn hide<T: 'static>(handle: &mut Option<WindowHandle<T>>, cx: &mut App) {
    if let Some(w) = handle.take() {
        let _ = w.update(cx, |_, w, _| w.remove_window());
    }
}

fn startup_settings(store: &mut Store) -> Result<Settings, String> {
    let cfg = store.load()?;
    if cfg.provider.api_key.trim().is_empty() {
        let mut cfg = cfg;
        cfg.provider.api_key.clear();
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
    super::diagnostics::event("startup_phase", serde_json::json!({"phase":"instance"}));
    super::pair::helper()?;
    super::diagnostics::event("startup_phase", serde_json::json!({"phase":"pair"}));
    let store = Arc::new(Mutex::new(Store::open()?));
    let loaded = {
        let mut store = store.lock().unwrap();
        let loaded = startup_settings(&mut store);
        if loaded.is_ok() {
            let _ = store.cleanup();
        }
        loaded
    };
    super::diagnostics::event("startup_phase", serde_json::json!({"phase":"settings"}));
    let mut valid = loaded.is_ok();
    let mut initial_error = loaded.as_ref().err().cloned().unwrap_or_default();
    let initial = loaded.unwrap_or_default();
    let pool = Pool::new();
    let speech = super::speech::Speech::new();
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
    super::diagnostics::event("startup_phase", serde_json::json!({"phase":"gpui"}));
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
  cx.spawn(async move|cx|{
   let(_host,_desktop)=(host,desktop);let mut config_valid=valid;let mut epoch=native::EPOCH.load(Ordering::Acquire);let mut trigger=0;let mut id=0;let mut generation=0;let mut pending:Option<(u64,Purpose)>=None;let mut cache:Option<(Selection,Instant)>=None;let mut last_source:Option<Source>=None;let mut mailbox:Option<(u64,Mailbox)>=None;let mut dragging:Option<([i32;2],[i32;2])>=None;let mut saving=false;let mut request_snapshot=false;let mut request_epoch=0;let mut hotkey_requested=false;let mut copy_requested=false;let mut quiet_at=Instant::now();let mut system_epoch=native::SYSTEM_EPOCH.load(Ordering::Acquire);let mut edits:VecDeque<(Entity<TextInput>,ClipboardIntent)>=VecDeque::new();let mut edit_active:Option<(u64,Entity<TextInput>,ClipboardIntent)>=None;let mut failed_copy:Option<Instant>=None;let mut last_applied_edit:Option<(Entity<TextInput>,u64,u64,Instant)>=None;let mut last_settings_status:Option<String>=if initial_error.is_empty(){None}else{Some(initial_error.clone())};
   loop {
    let context=native::SYSTEM_EPOCH.load(Ordering::Acquire);if context!=system_epoch{system_epoch=context;edits.clear();edit_active=None;failed_copy=None;last_applied_edit=None;pending=None;cache=None;generation+=1;pool.cancel_pending();mailbox=None;request_snapshot=false;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);if let Some(f)=form{let _=f.update(cx,|f,w,cx|{let _=native::keep_accessible(w);f.status="系统状态已变化，未完成的剪贴板操作已取消".into();cx.notify();});}});}
    let changed=native::EPOCH.load(Ordering::Acquire);
    if let Some(source)=util::current(changed){last_source=Some(source);}
    if changed!=epoch{epoch=changed;quiet_at=Instant::now();pending=None;cache=None;if !request_snapshot{generation+=1;pool.cancel_pending();mailbox=None;}let _=cx.update(|cx|{hide(&mut bee,cx);if panel.is_some_and(|w|w.update(cx,|p,_,_|!p.snapshot).unwrap_or(false)){hide(&mut panel,cx);}});}
    if panel.is_some_and(|p|p.update(cx,|p,_,_|!p.snapshot&&!util::is_current(p.selection.source)).unwrap_or(false)){generation+=1;pool.cancel_pending();mailbox=None;let _=cx.update(|cx|hide(&mut panel,cx));}
    if cache.as_ref().is_some_and(|(s,at)|at.elapsed()>Duration::from_millis(TTL_MS)||!util::is_current(s.source)){cache=None;let _=cx.update(|cx|hide(&mut bee,cx));}
    let t=native::triggered();if edit_active.is_none()&&edits.is_empty()&&cfg.selection_enabled&&!hotkey_requested&&!copy_requested&&pending.as_ref().is_none_or(|(_,p)|matches!(p,Purpose::Detect))&&t!=trigger&&native::quiet_since(t){trigger=t;if let Some(source)=util::current(epoch){id+=1;coordinator.request(id,source,epoch,util::cursor(),false);pending=Some((id,Purpose::Detect));}}
    while let Ok(event)=erx.try_recv(){match event{
     Event::Settings|Event::About=>{let about=matches!(event,Event::About);let info=format!("BeeK {} · Windows x64 · helper {} · {}",env!("CARGO_PKG_VERSION"),env!("BEEK_BUILD_ID").get(..12).unwrap_or("unknown"),coordinator.snapshot());let _=cx.update(|cx|{hide(&mut bee,cx);let active=form.and_then(|f|f.update(cx,|_,w,_|native::activate(w)).ok());if active.is_none(){form=open_form(cx,tx.clone(),cfg.clone(),if about{info.clone()}else{last_settings_status.clone().unwrap_or_default()});}if about{if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=info;cx.notify();});}}});},
     Event::IpcUnavailable=>{let message="实例通信暂不可用，请从托盘退出后重新启动".to_owned();last_settings_status=Some(message.clone());let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});});},
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
    if let Some((g,result))=speech.poll(){if g==generation{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status=result.map(|_|"朗读完成").unwrap_or_else(|e|e).into();cx.notify();});});}}
    let restart=instance.restart_events.as_ref().and_then(|rx|rx.try_recv().ok());
    if let Some(event)=restart{match event{
     super::lifecycle::RestartEvent::Ready=>instance.grant_restart(),
     super::lifecycle::RestartEvent::Committed=>{instance.commit_exit();super::lifecycle::exit_deadline();let _=cx.update(|cx|cx.quit());return;},
     super::lifecycle::RestartEvent::Failed(e)=>{instance.restart_events=None;let _=cx.update(|cx|{if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=e;cx.notify();});}else{form=open_form(cx,tx.clone(),cfg.clone(),e);}});}
    }}
    let outcomes={coordinator.result.lock().unwrap().drain(..).collect::<Vec<_>>()};
    for outcome in outcomes{
     let reply=match &outcome{Outcome::Selected(id,_)|Outcome::Empty(id,_)|Outcome::Fault(id,_)|Outcome::Copied(id,_)=>Some(*id),_=>None};
     if edit_active.as_ref().is_some_and(|(id,_,_)|Some(*id)==reply){
      let (_,field,intent)=edit_active.take().unwrap();
      let mut applied=matches!((&intent,&outcome),(ClipboardIntent::Copy{cut:false,..},Outcome::Copied(..)));
      if let Some(f)=form{let _=f.update(cx,|f,w,cx|{
       if f.fields.contains(&field){
        let updated=match (&intent,&outcome){
         (ClipboardIntent::Paste{ticket,..},Outcome::Selected(_,selection))=>field.update(cx,|field,cx|field.paste_value(*ticket,&selection.text,w,cx)).map(|revision|(*ticket,revision)),
         (ClipboardIntent::Copy{ticket,range,cut:true,..},Outcome::Copied(..))=>field.update(cx,|field,cx|field.cut_complete(*ticket,range.clone(),w,cx)).map(|revision|(*ticket,revision)),_=>None
        };
        if let Some((old,revision))=updated{applied=true;last_applied_edit=Some((field.clone(),old,revision,Instant::now()));for(target,intent)in &mut edits{if target==&field{if let ClipboardIntent::Paste{ticket,..}=intent{if *ticket==old{*ticket=revision;}}}}}
       }
       f.status=if applied{"剪贴板操作已完成"}else if matches!(outcome,Outcome::Copied(..)){"已复制；输入位置已变化，未执行剪切"}else{"输入位置、组合文字或剪贴板已变化，操作未完成，请重试"}.into();cx.notify();
      });}
      #[cfg(feature="test-fixtures")]super::diagnostics::event("editor_result",serde_json::json!({"id":reply,"applied":applied}));
      if !matches!(outcome,Outcome::Copied(..)){if let ClipboardIntent::Copy{..}=intent{failed_copy=Some(Instant::now());edits.retain(|(_,e)|!matches!(e,ClipboardIntent::Paste{..}));}}
      continue;
     }
     match outcome{
     Outcome::Copied(reply,_) if pending.as_ref().is_some_and(|(expected,purpose)|*expected==reply&&matches!(purpose,Purpose::Copy))=>{
      pending=None;let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.copied_until=Some(Instant::now()+Duration::from_millis(1500));p.status="完成".into();cx.notify();cx.spawn(async move|this,cx|{cx.background_executor().timer(Duration::from_millis(1500)).await;let _=this.update(cx,|_,cx|cx.notify());}).detach();});});
     },
     Outcome::Selected(reply,s) if pending.as_ref().is_some_and(|(expected,_)|*expected==reply)=>{
      let purpose=pending.take().unwrap().1;
      if s.epoch==epoch&&s.epoch==native::EPOCH.load(Ordering::Acquire)&&(util::is_current(s.source)||matches!(purpose,Purpose::Clipboard(false))){match purpose{
       Purpose::Copy=>{},
       Purpose::Detect=>{cache=Some((s.clone(),Instant::now()));let _=cx.update(|cx|{hide(&mut bee,cx);bee=open_bee(cx,tx.clone(),s);});},
       Purpose::Verify(old)=>if old.same_selection(&s){let _=tx.try_send(Command::Translate(s,false));}else{let _=cx.update(|cx|hide(&mut bee,cx));},
       Purpose::Preview|Purpose::Clipboard(false)=>{generation+=1;pool.cancel_pending();mailbox=None;request_snapshot=false;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);panel=open_panel(cx,tx.clone(),s,true);});},
       Purpose::Clipboard(true)=>{cache=Some((s.clone(),Instant::now()));let _=cx.update(|cx|{hide(&mut bee,cx);bee=open_bee(cx,tx.clone(),s);});}
      }}
     },
     Outcome::Copied(reply,_)=>{if pending.as_ref().is_some_and(|(i,_)|*i==reply){pending=None;let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="已复制译文".into();cx.notify();});});}},
     Outcome::Empty(reply,status)=>{if pending.as_ref().is_some_and(|(i,_)|*i==reply){let purpose=pending.take().unwrap().1;let copy=matches!(purpose,Purpose::Copy);let manual=matches!(purpose,Purpose::Preview|Purpose::Clipboard(false));let _=cx.update(|cx|{hide(&mut bee,cx);if copy{if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，剪贴板可能已变化或被占用，请重试".into();cx.notify();});}}if manual{let message=selection_status(status).to_owned();if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});}else{form=open_form(cx,tx.clone(),cfg.clone(),message);}}});}},
     Outcome::Fault(reply,code)=>{let _=code;if pending.as_ref().is_some_and(|(i,_)|*i==reply){let purpose=pending.take().unwrap().1;let copy=matches!(purpose,Purpose::Copy);let manual=matches!(purpose,Purpose::Preview|Purpose::Clipboard(false));if manual{last_settings_status=Some("选区或剪贴板检测超时、繁忙或已取消，请重试".into());}let _=cx.update(|cx|{hide(&mut bee,cx);if copy{if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，请稍后重试".into();cx.notify();});}}if manual{let message=last_settings_status.clone().unwrap();if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});}else{form=open_form(cx,tx.clone(),cfg.clone(),message);}}});}},
     Outcome::Paused=>{let _=cx.update(|cx|{hide(&mut bee,cx);if let Some(f)=form{let _=f.update(cx,|s,_,cx|{s.status="选区检测已暂停，请从托盘手动恢复".into();cx.notify();});}});},_=>{}
    }}
    while let Ok(command)=rx.try_recv(){match command{
     Command::Click(s)=>if cache.as_ref().is_some_and(|(c,at)|c.same_selection(&s)&&at.elapsed()<Duration::from_millis(TTL_MS))&&s.epoch==epoch&&util::is_current(s.source){id+=1;let point=s.rects.last().map(|r|[(r.x+r.width/2.) as i32,(r.y+r.height/2.) as i32]).unwrap_or_else(util::cursor);coordinator.request(id,s.source,epoch,point,s.clipboard_sequence.is_some());pending=Some((id,Purpose::Verify(s)));},
     Command::Translate(s,snapshot)=>{
      if snapshot{let valid=panel.is_some_and(|p|p.update(cx,|p,_,_|p.snapshot&&p.selection.same_selection(&s)).unwrap_or(false));if !valid{continue;}}
      else if s.epoch!=native::EPOCH.load(Ordering::Acquire)||!util::is_current(s.source){continue;}
      generation+=1;pool.cancel_pending();request_snapshot=snapshot;request_epoch=s.epoch;let g=generation;let config=cfg.clone();let text=s.text.clone();
      let _=cx.update(|cx|{hide(&mut bee,cx);if !snapshot{hide(&mut panel,cx);panel=open_panel(cx,tx.clone(),s,false);}if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=true;p.error=false;p.copied_until=None;p.status="正在翻译…".into();p.body.clear();cx.notify();});}});
      let error=if saving{Some("设置正在保存，请稍后重新选择或重试")}else if !config_valid{Some("配置损坏，恢复并保存设置前不能发送翻译请求")}else if work_tx.try_send(Work::Request(g,config,text)).is_err(){Some("设置操作繁忙，请稍后重试")}else{None};
      if let Some(error)=error{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=false;p.error=true;p.status=error.into();cx.notify();});});}
     },
     Command::Save(next,key) if !saving=>{
      saving=if edit_active.is_some()||!edits.is_empty(){false}else{work_tx.try_send(Work::Save(cfg.clone(),next,key)).is_ok()};if !saving{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.busy=false;f.status=if edit_active.is_some()||!edits.is_empty(){"请等待剪贴板编辑完成后再保存"}else{"设置操作繁忙，请稍后重试"}.into();cx.notify();});});}
     },
     Command::Copy(text,sequence)=>{if let Some(source)=util::source(unsafe{windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()},epoch){id+=1;coordinator.copy(id,source,epoch,text,sequence);pending=Some((id,Purpose::Copy));}else{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，请返回普通桌面后重试".into();cx.notify();});});}},
     Command::Edit(field,mut intent)=>{
      #[cfg(feature="test-fixtures")]super::diagnostics::event("editor_intent",match &intent{ClipboardIntent::Paste{ticket,..}=>serde_json::json!({"kind":"paste","ticket":ticket}),ClipboardIntent::Copy{ticket,cut,..}=>serde_json::json!({"kind":if *cut{"cut"}else{"copy"},"ticket":ticket})});if let ClipboardIntent::Paste{ticket,at,..}=&mut intent{if let Some((target,old,new,completed))=&last_applied_edit{if target==&field&&*ticket==*old&&*at<=*completed{*ticket=*new;}}}if edits.len()<4{let ignore=matches!(&intent,ClipboardIntent::Paste{at,..} if failed_copy.is_some_and(|failed|*at<=failed));if !ignore{edits.push_back((field,intent));}else{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status="先前复制未完成，关联粘贴已取消，请重试".into();cx.notify();});});}}else{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status="剪贴板操作繁忙，请稍后重试".into();cx.notify();});});}},
     Command::Speak(text)=>{let result=speech.speak(generation,text);let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status=result.map(|_|"正在朗读…").unwrap_or_else(|e|e).into();cx.notify();});});},Command::Close=>{generation+=1;pool.cancel_pending();mailbox=None;let _=cx.update(|cx|{hide(&mut panel,cx);hide(&mut bee,cx);});},
     Command::Update=>{let queued=super::integration::open_updates();let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=if queued{"正在检查 Windows 稳定版更新…"}else{"更新检查繁忙，请稍后重试"}.into();cx.notify();});});},
     Command::Drag(at)=>{if let Some(p)=panel{let mut origin=None;let _=p.update(cx,|p,w,_|{if !p.snapshot{if let Ok(h)=native::hwnd(w){let mut rect=windows::Win32::Foundation::RECT::default();if unsafe{windows::Win32::UI::WindowsAndMessaging::GetWindowRect(h,&mut rect)}.is_ok(){origin=Some([rect.left,rect.top]);}}}});if let Some(origin)=origin{dragging=Some((at,origin));}}},_=>{}
    }}
    if edit_active.is_none(){if let Some((field,intent))=edits.pop_front(){
     let source=form.and_then(|f|f.update(cx,|_,w,_|native::hwnd(w).ok().and_then(|h|util::source(h,epoch))).ok().flatten());
     if let Some(source)=source{id+=1;match &intent{ClipboardIntent::Paste{sequence,..}=>coordinator.paste(id,source,epoch,*sequence),ClipboardIntent::Copy{text,sequence,..}=>coordinator.copy(id,source,epoch,text.clone(),*sequence)}edit_active=Some((id,field,intent));}else{edits.clear();let message="设置窗口已不可用，未完成的输入操作已取消".to_owned();last_settings_status=Some(message.clone());let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});});}
    }}
    while let Ok(message)=bg_rx.try_recv(){match message{
     Background::Saved(result,warning)=>{saving=false;let committed=result.is_ok();let status=match result{Ok(saved)=>{cfg=saved;config_valid=true;_desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);warning.unwrap_or_else(||"已保存".into())},Err(e)=>e};last_settings_status=Some(status.clone());let editing=edit_active.is_some()||!edits.is_empty();let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{let signature=f.save_revision.take();let unchanged=signature.as_ref().is_some_and(|s|s==&f.signature(cx))&&!editing;f.busy=false;f.status=status;if committed{if unchanged{f.cfg=cfg.clone();}else{f.status.push_str("；提交后的新修改尚未保存");}f.cfg.provider.credential_ref=cfg.provider.credential_ref.clone();f.fields[0].update(cx,|key,cx|key.set_saved_secret(cfg.provider.credential_ref.is_some(),cx));if signature.as_ref().is_some_and(|(_,flags)|flags[4]==f.clear){f.clear=false;}if !editing&&signature.as_ref().is_some_and(|(fields,_)|fields[0]==f.fields[0].read(cx).revision()){f.fields[0].update(cx,|key,cx|key.set_value("",cx));}}cx.notify();});});},
     Background::Request(g,result) if g==generation&&(request_snapshot||request_epoch==native::EPOCH.load(Ordering::Acquire))&&panel.is_some_and(|p|p.update(cx,|_,_,_|()).is_ok())=>match result{Ok(request)=>mailbox=Some((g,pool.submit(request))),Err(e)=>{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=false;p.error=true;p.status=e;cx.notify();});});}},_=>{}
    }}
    if let Some(status)=super::integration::update_result(){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=status;cx.notify();});});}
    if !native::HOTKEY_OK.load(Ordering::Acquire){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{if !f.status.contains("快捷键"){f.status="全局快捷键注册失败（冲突或格式无效），请修改后保存".into();cx.notify();}});});}
    let mut completed=false;
    if let Some((g,m))=&mailbox{if *g==generation{let progress=std::mem::replace(&mut *m.lock().unwrap(),Progress::Idle);completed=matches!(progress,Progress::Done(_));if !matches!(progress,Progress::Idle){let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{match progress{Progress::Idle=>{},Progress::Loading=>{p.status="正在翻译…".into();},Progress::Queued=>{p.status="等待前面的实际请求结束…".into();},Progress::Text(text)=>{p.status="正在翻译…".into();p.body=text;},Progress::Done(Ok(text))=>{p.active=false;p.error=false;p.body=text;p.status="完成".into();},Progress::Done(Err(e))=>{p.active=false;p.error=true;p.status=e;}}cx.notify();});});}}}
    if completed{mailbox=None;}
    if let Some((start,origin))=dragging{if unsafe{windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(1)}>=0{dragging=None;}else{let point=util::cursor();if let Some(p)=panel{let _=p.update(cx,|_,w,_|native::move_popup(w,origin[0]+point[0]-start[0],origin[1]+point[1]-start[1]));}}}
    cx.background_executor().timer(Duration::from_millis(16)).await;
   }
  }).detach();
 });
    Ok(())
}
