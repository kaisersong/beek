use super::onboarding;
use super::{
    input::{self, ClipboardIntent, TextInput},
    native::{self, Event},
    selection::{Coordinator, Outcome},
    store::Store,
    util,
};
use crate::core::{
    localization::{self, tr, UiLanguage},
    translation_cache::CacheSettings,
    translation_engine::{self as engines, Engine},
    translation_language::TargetLanguage,
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
use std::collections::BTreeMap;
use std::{
    collections::VecDeque,
    sync::{atomic::Ordering, mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

pub(super) enum Command {
    Click(Selection),
    Translate(Selection, bool),
    Copy(String, u32),
    Edit(Entity<TextInput>, ClipboardIntent),
    Speak(String),
    Close,
    Save(Settings, KeyEdit, bool),
    Update,
    Demo,
    OpenGuide,
    Guide(onboarding::Action),
    Drag([i32; 2]),
}
pub(super) type Tx = mpsc::SyncSender<Command>;
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
    engine: Engine,
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
        (self.engine == Engine::Ark && crate::core::prompts::is_single_word(&self.selection.text))
            .then(|| self.selection.text.trim())
    }
    fn pron_and_body(&self) -> (Option<String>, String) {
        if self.word().is_some() && !self.error {
            stream::split_pron(&self.body)
        } else {
            (None, self.body.clone())
        }
    }
    fn can_copy(&self) -> bool {
        !self.error && !self.body.trim().is_empty()
    }
    fn copy_text(&self) -> Option<String> {
        if !self.can_copy() {
            return None;
        }
        let (pron, body) = self.pron_and_body();
        Some(match (self.word(), pron) {
            (Some(word), Some(pron)) => format!("{word}  {pron}\n{body}"),
            (Some(word), None) => format!("{word}\n{body}"),
            _ => body,
        })
    }
    fn copy(&mut self, _: &CopyPreview, _: &mut Window, _: &mut Context<Self>) {
        self.copy_result();
    }
    fn copy_result(&self) {
        if let Some(text) = self.copy_text() {
            let _ = self.tx.try_send(Command::Copy(text, unsafe {
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
        let (pron, body) = self.pron_and_body();
        let word = self.word().map(str::to_owned);
        let shown = if preview {
            self.selection.text.clone()
        } else if self.error {
            localization::message("翻译失败：{}", &[tr(&self.status)])
        } else if self.body.is_empty() {
            if self.active && self.status != "正在翻译…" {
                tr(&self.status).to_owned()
            } else if word.is_some() {
                tr("查询中…").into()
            } else {
                tr("翻译中…").into()
            }
        } else {
            body
        };
        let footer = if self.status.starts_with("复制未完成") {
            self.status.clone()
        } else if preview {
            self.status.clone()
        } else {
            format!(
                "{} · {}",
                tr(self.engine.label()),
                tr("点击弹窗外任意处关闭")
            )
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
        let copied = self.can_copy()
            && self
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
                                .children(
                                    pron.clone().map(|p| {
                                        div().text_sm().text_color(rgb(0x666666)).child(p)
                                    }),
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
                                .child(tr("翻译 (Enter)"))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, window, cx| {
                                        this.translate(&TranslateSnapshot, window, cx);
                                    }),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id("close-card")
                            .cursor_pointer()
                            .px_1()
                            .child("×")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|s, _, _, _| {
                                    let _ = s.tx.try_send(Command::Close);
                                }),
                            ),
                    )
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
    save_revision: Option<(Vec<u64>, [bool; 6])>,
    fields: Vec<Entity<TextInput>>,
    cfg: Settings,
    status: String,
    busy: bool,
    clear: bool,
    tx: Tx,
    drafts: BTreeMap<String, (String, bool)>,
    guide_mode: bool,
    focus: FocusHandle,
}
impl Form {
    fn visible_fields(&self) -> Vec<usize> {
        let mut fields = match self.cfg.translation_engine {
            Engine::Ark => vec![0, 1, 2],
            Engine::Azure => vec![0, 5],
            Engine::Baidu => vec![0, 4],
            Engine::Google => vec![0],
            Engine::MyMemory => vec![6, 7],
            Engine::LibreTranslate => vec![0, 2],
            _ => vec![],
        };
        if !self.guide_mode {
            fields.extend([3, 8, 9]);
        }
        fields
    }
    fn capture(&mut self, cx: &App) {
        let p = self.cfg.active_provider_mut();
        p.model = self.fields[1].read(cx).value().trim().into();
        if p.model.is_empty() {
            p.model = crate::core::prompts::DEFAULT_MODEL.into();
        }
        p.base_url = self.fields[2].read(cx).value().trim().into();
        p.app_id = self.fields[4].read(cx).value().trim().into();
        p.region = self.fields[5].read(cx).value().trim().into();
        p.source_language = self.fields[6].read(cx).value().trim().into();
        p.email = self.fields[7].read(cx).value().trim().into();
        self.cfg.hotkey = self.fields[3].read(cx).value().trim().into();
        self.drafts.insert(
            self.cfg.translation_engine.key().into(),
            (self.fields[0].read(cx).value(), self.clear),
        );
    }
    fn switch_engine(&mut self, engine: Engine, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || engine == self.cfg.translation_engine {
            return;
        }
        self.capture(cx);
        self.cfg.translation_engine = engine;
        let p = self.cfg.active_provider();
        let (key, clear) = self.drafts.get(engine.key()).cloned().unwrap_or_default();
        self.clear = clear;
        for (i, value) in [
            (0, key),
            (1, p.model),
            (2, p.base_url),
            (4, p.app_id),
            (5, p.region),
            (6, p.source_language),
            (7, p.email),
        ] {
            self.fields[i].update(cx, |f, cx| f.set_value(&value, cx));
        }
        self.fields[0].update(cx, |f, cx| {
            f.set_saved_secret(p.credential_ref.is_some(), cx)
        });
        self.status.clear();
        if let Some(index) = self.visible_fields().first() {
            self.fields[*index].read(cx).focus(window);
        } else {
            window.focus(&self.focus);
        }
        cx.notify();
    }
    fn signature(&self, cx: &App) -> (Vec<u64>, [bool; 6]) {
        (
            self.fields
                .iter()
                .map(|f| f.read(cx).revision())
                .chain([
                    self.cfg.translation_engine as u64,
                    self.cfg.translation_target as u64,
                    self.cfg.ui_language as u64,
                ])
                .collect(),
            [
                self.cfg.selection_enabled,
                self.cfg.copy_to_popup,
                self.cfg.hotkey_enabled,
                self.cfg.autostart,
                self.clear,
                self.cfg.auto_update,
            ],
        )
    }
    fn next(&mut self, _: &NextField, w: &mut Window, cx: &mut Context<Self>) {
        let fields = self.visible_fields();
        if fields.is_empty() {
            w.focus(&self.focus);
            return;
        }
        let n = fields
            .iter()
            .position(|i| self.fields[*i].focus_handle(cx).is_focused(w))
            .unwrap_or(0);
        self.fields[fields[(n + 1) % fields.len()]]
            .read(cx)
            .focus(w);
    }
    fn previous(&mut self, _: &PreviousField, w: &mut Window, cx: &mut Context<Self>) {
        let fields = self.visible_fields();
        if fields.is_empty() {
            w.focus(&self.focus);
            return;
        }
        let n = fields
            .iter()
            .position(|i| self.fields[*i].focus_handle(cx).is_focused(w))
            .unwrap_or(0);
        self.fields[fields[(n + fields.len() - 1) % fields.len()]]
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
        self.capture(cx);
        let mut cfg = self.cfg.clone();

        let cache = CacheSettings::from_input(
            self.fields[8].read(cx).value().as_str(),
            self.fields[9].read(cx).value().as_str(),
        );
        match cache {
            Ok(cache) => cfg.translation_cache = cache,
            Err(error) => {
                self.status = error;
                cx.notify();
                return;
            }
        }
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
        if self
            .tx
            .try_send(Command::Save(cfg, edit, self.guide_mode))
            .is_ok()
        {
            self.busy = true;
            self.save_revision = Some(self.signature(cx));
            self.status = "正在保存…".into();
            cx.notify();
        }
    }
}
impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let provider = self.cfg.active_provider();
        let labels = [
            if provider.credential_ref.is_some() {
                "API Key（已保存；输入新 Key 可替换）"
            } else {
                "API Key（输入后显示为星号）"
            },
            "模型 / 推理接入点",
            "服务地址",
            "全局快捷键（默认 Ctrl+Alt+T）",
            "百度 App ID",
            "Azure 区域",
            "源语言（例如 en）",
            "邮箱（可选）",
            "缓存条数（0–1000；0 关闭）",
            "缓存有效分钟（0–1440）",
        ];
        let mut view = div()
            .id("settings")
            .size_full()
            .p_6()
            .bg(rgb(0xf8f9fa))
            .text_color(rgb(0x25282d))
            .flex()
            .flex_col()
            .gap_3()
            .overflow_y_scroll()
            .key_context("BeeKSettings")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::next))
            .on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::save))
            .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(tr(
                if self.guide_mode {
                    "翻译服务设置"
                } else {
                    "BeeK 设置"
                },
            )))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x666b73))
                    .child(tr("翻译服务")),
            );
        let mut providers = div().flex().flex_wrap().gap_2();
        for engine in Engine::ALL {
            providers = providers.child(
                div()
                    .id(SharedString::from(format!("engine-{}", engine.key())))
                    .px_3()
                    .py_2()
                    .rounded(px(8.))
                    .text_size(px(13.))
                    .border_1()
                    .border_color(rgb(if self.cfg.translation_engine == engine {
                        0x869aaf
                    } else {
                        0xe0e4e8
                    }))
                    .bg(rgb(if self.cfg.translation_engine == engine {
                        0xe4edf5
                    } else {
                        0xffffff
                    }))
                    .cursor_pointer()
                    .child(tr(engine.label()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |s, _, w, cx| s.switch_engine(engine, w, cx)),
                    ),
            );
        }
        view = view.child(providers);
        if !self.guide_mode {
            view = view
                .child(div().text_sm().child(tr("翻译为")))
                .child(div().flex().flex_wrap().gap_2().children(
                    TargetLanguage::ALL.into_iter().map(|language| {
                        div()
                            .id(SharedString::from(format!("target-{}", language.key())))
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(if self.cfg.translation_target == language {
                                0xdce7f0
                            } else {
                                0xffffff
                            }))
                            .cursor_pointer()
                            .text_sm()
                            .child(tr(language.label()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |s, _, _, cx| {
                                    if !s.busy {
                                        s.cfg.translation_target = language;
                                        cx.notify();
                                    }
                                }),
                            )
                    }),
                ))
                .child(div().text_sm().child(tr("界面语言")))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .children(UiLanguage::ALL.into_iter().map(|language| {
                            div()
                                .id(SharedString::from(format!("ui-{:?}", language)))
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(if self.cfg.ui_language == language {
                                    0xdce7f0
                                } else {
                                    0xffffff
                                }))
                                .cursor_pointer()
                                .text_sm()
                                .child(language.label())
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |s, _, _, cx| {
                                        if !s.busy {
                                            s.cfg.ui_language = language;
                                            localization::configure(
                                                language,
                                                &super::util::system_language(),
                                            );
                                            cx.notify();
                                        }
                                    }),
                                )
                        })),
                );
        }
        for index in self.visible_fields() {
            view = view
                .child(div().text_sm().child(tr(labels[index])))
                .child(self.fields[index].clone());
        }
        if self.cfg.translation_engine == Engine::Ark && self.guide_mode {
            view = view.child(
                div()
                    .text_xs()
                    .text_color(rgb(0x666b73))
                    .child(tr("模型可留空，使用默认豆包模型。")),
            );
        }
        if self.cfg.translation_engine == Engine::PublicBaidu {
            view = view.child(div().text_sm().child(tr(
                "免费公共服务无需 Key，单次最多 1500 字节；该服务不缓存译文。",
            )));
        }
        if self.cfg.translation_engine == Engine::LibreTranslate {
            view=view.child(div().text_sm().child(tr("本地默认地址为 http://127.0.0.1:5000/translate，请先安装并启动 LibreTranslate / Argos 服务。")));
        }
        let flags = if self.guide_mode {
            vec![]
        } else {
            vec![
                ("选中文本后显示蜜蜂按钮", self.cfg.selection_enabled, 0),
                ("复制文本后显示（默认关闭）", self.cfg.copy_to_popup, 1),
                ("启用全局快捷键", self.cfg.hotkey_enabled, 2),
                ("登录 Windows 时启动", self.cfg.autostart, 3),
                ("清除已保存的 API Key", self.clear, 4),
                ("自动检查更新", self.cfg.auto_update, 5),
            ]
        };
        for (label, on, index) in flags {
            if index == 4
                && matches!(
                    self.cfg.translation_engine,
                    Engine::PublicBaidu | Engine::MyMemory
                )
            {
                continue;
            }
            view = view.child(
                div()
                    .id(index)
                    .cursor_pointer()
                    .text_sm()
                    .child(format!("{} {}", if on { "☑" } else { "☐" }, tr(label)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |s, _, _, cx| {
                            if s.busy {
                                return;
                            }
                            match index {
                                0 => s.cfg.selection_enabled = !s.cfg.selection_enabled,
                                1 => s.cfg.copy_to_popup = !s.cfg.copy_to_popup,
                                2 => s.cfg.hotkey_enabled = !s.cfg.hotkey_enabled,
                                3 => s.cfg.autostart = !s.cfg.autostart,
                                4 => s.clear = !s.clear,
                                _ => s.cfg.auto_update = !s.cfg.auto_update,
                            }
                            cx.notify();
                        }),
                    ),
            );
        }
        view = view
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x795a22))
                    .child(tr(&self.status).to_owned()),
            )
            .child(
                div()
                    .id("save")
                    .p_3()
                    .rounded(px(10.))
                    .bg(rgb(0x232627))
                    .text_color(white())
                    .cursor_pointer()
                    .child(tr(if self.busy {
                        "保存中…"
                    } else if self.guide_mode {
                        "保存并启用"
                    } else {
                        "保存 (Ctrl+S)"
                    }))
                    .on_mouse_down(MouseButton::Left, cx.listener(|s, _, _, cx| s.submit(cx))),
            );
        if self.guide_mode {
            return view.child(
                div()
                    .id("cancel-service")
                    .p_2()
                    .text_sm()
                    .text_color(rgb(0x666b73))
                    .cursor_pointer()
                    .child(tr("取消"))
                    .on_mouse_down(MouseButton::Left, |_, w, _| w.remove_window()),
            );
        }
        view.child(
            div()
                .id("try-translation")
                .cursor_pointer()
                .text_sm()
                .child(tr("点击试译"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, _| {
                        let _ = s.tx.try_send(Command::Demo);
                    }),
                ),
        )
        .child(
            div()
                .id("show-guide")
                .cursor_pointer()
                .text_sm()
                .child(tr("使用引导"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, _| {
                        let _ = s.tx.try_send(Command::OpenGuide);
                    }),
                ),
        )
        .child(
            div()
                .id("updates")
                .cursor_pointer()
                .text_sm()
                .child(localization::message(
                    "BeeK {} · 手动检查更新",
                    &[env!("CARGO_PKG_VERSION")],
                ))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, _| {
                        let _ = s.tx.try_send(Command::Update);
                    }),
                ),
        )
        .child(div().text_xs().text_color(rgb(0x666b73)).child(tr(
            "仅点击翻译后发送所选文本。剪贴板回退不会模拟 Ctrl+C；不自动下载安装更新。",
        )))
    }
}

fn open_form(cx: &mut App, tx: Tx, cfg: Settings, status: String) -> Option<WindowHandle<Form>> {
    open_form_mode(cx, tx, cfg, status, false)
}
fn open_form_mode(
    cx: &mut App,
    tx: Tx,
    cfg: Settings,
    status: String,
    guide_mode: bool,
) -> Option<WindowHandle<Form>> {
    let handle = cx
        .open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(
                        tr(if guide_mode {
                            "翻译服务设置"
                        } else {
                            "BeeK 设置"
                        })
                        .into(),
                    ),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    if guide_mode {
                        size(px(640.), px(650.))
                    } else {
                        size(px(720.), px(780.))
                    },
                    cx,
                ))),
                ..Default::default()
            },
            |w, cx| {
                let mut fields = Vec::new();
                let provider = cfg.active_provider();
                for (i, (label, value)) in [
                    ("API Key", ""),
                    ("model", provider.model.as_str()),
                    ("API URL", provider.base_url.as_str()),
                    ("hotkey", cfg.hotkey.as_str()),
                    ("App ID", provider.app_id.as_str()),
                    ("region", provider.region.as_str()),
                    ("source language", provider.source_language.as_str()),
                    ("email", provider.email.as_str()),
                    (
                        "cache entries",
                        &cfg.translation_cache.max_entries.to_string(),
                    ),
                    (
                        "cache minutes",
                        &cfg.translation_cache.ttl_minutes.to_string(),
                    ),
                ]
                .into_iter()
                .enumerate()
                {
                    let field = cx.new(|cx| {
                        let mut f = TextInput::new(cx, label);
                        f.set_value(value, cx);
                        f.set_compact(guide_mode, cx);
                        if i == 0 {
                            f.mask();
                            f.set_saved_secret(provider.credential_ref.is_some(), cx);
                        }
                        f
                    });
                    fields.push(field);
                }
                let first = match cfg.translation_engine {
                    Engine::PublicBaidu | Engine::Unsupported => 3,
                    Engine::MyMemory => 6,
                    _ => 0,
                };
                if !guide_mode
                    || !matches!(
                        cfg.translation_engine,
                        Engine::PublicBaidu | Engine::Unsupported
                    )
                {
                    fields[first].read(cx).focus(w);
                }
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    if guide_mode
                        && matches!(
                            cfg.translation_engine,
                            Engine::PublicBaidu | Engine::Unsupported
                        )
                    {
                        w.focus(&focus);
                    }
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
                        guide_mode,
                        focus,
                        drafts: BTreeMap::new(),
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
                        engine: Engine::Ark,
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
/// Runs only in a fixture build on the interactive desktop. Rejected copies
/// must not enqueue a write or change the actual clipboard sequence number.
#[cfg(feature = "test-fixtures")]
pub fn copy_result_selftest() {
    Application::new().run(|cx| {
        let (tx, rx) = mpsc::sync_channel(8);
        let panel = cx.new(|cx| Panel {
            active: false,
            engine: Engine::Ark,
            focus: cx.focus_handle(),
            selection: Selection {
                source: Source {
                    hwnd: 0,
                    pid: 1,
                    created: 1,
                    generation: 1,
                },
                epoch: 1,
                runtime_id: vec![],
                text: "word".into(),
                rects: vec![],
                clipboard_sequence: None,
            },
            body: String::new(),
            status: String::new(),
            error: false,
            copied_until: None,
            anchor: None,
            layout_height: 0.,
            snapshot: false,
            tx,
        });
        let mut passed = 0;
        for (body, error, active) in [
            ("", false, true),
            ("error", true, false),
            ("", false, false),
            (" \n\t\u{3000}", false, false),
        ] {
            let before =
                unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() };
            panel.update(cx, |panel, _| {
                panel.body = body.into();
                panel.error = error;
                panel.active = active;
                panel.copied_until = Some(Instant::now() + Duration::from_secs(1));
                assert!(!panel.can_copy());
                panel.copy_result();
            });
            assert!(rx.try_recv().is_err());
            assert_eq!(
                unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() },
                before
            );
            assert!(!panel.read(cx).can_copy());
            passed += 1;
        }
        for (engine, text, raw, pron, body, expected) in [
            (
                Engine::Ark,
                "word",
                "读音：/wɜːd/\n- n. 单词",
                Some("/wɜːd/"),
                "- n. 单词",
                "word  /wɜːd/\n- n. 单词",
            ),
            (
                Engine::Ark,
                "word",
                "读音：/wɜ",
                None,
                "读音：/wɜ",
                "word\n读音：/wɜ",
            ),
            (
                Engine::Ark,
                "word",
                "- n. 单词",
                None,
                "- n. 单词",
                "word\n- n. 单词",
            ),
            (
                Engine::Ark,
                "word",
                "读音：/wɜːd/\n",
                Some("/wɜːd/"),
                "",
                "word  /wɜːd/\n",
            ),
            (
                Engine::Ark,
                "two words",
                "读音：ordinary translation\n正文",
                None,
                "读音：ordinary translation\n正文",
                "读音：ordinary translation\n正文",
            ),
            (
                Engine::LibreTranslate,
                "word",
                "读音：ordinary translation\n正文",
                None,
                "读音：ordinary translation\n正文",
                "读音：ordinary translation\n正文",
            ),
        ] {
            panel.update(cx, |panel, _| {
                panel.engine = engine;
                panel.selection.text = text.into();
                panel.body = raw.into();
                panel.error = false;
                assert_eq!(
                    panel.pron_and_body(),
                    (pron.map(str::to_owned), body.into())
                );
                assert_eq!(panel.copy_text().as_deref(), Some(expected));
                panel.copy_result();
            });
            assert!(matches!(rx.try_recv().unwrap(), Command::Copy(value, _) if value == expected));
            passed += 1;
        }
        println!(
            "{{\"passed\":{},\"rejected_clipboard_unchanged\":true,\"real_panel_entry\":true}}",
            passed
        );
        cx.quit();
    });
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
            text: if state == "paragraph" || state == "long" || state == "text-prefix" {
                "Selection translation should feel instant and unobtrusive.".into()
            } else { "serendipity".into() },
            rects: vec![Rect { x: 400., y: if state == "edge" { 990. } else { 300. }, width: 180., height: 22. }],
            clipboard_sequence: None,
        };
        if state.starts_with("onboarding")||state.starts_with("welcome") {
            let mut cfg=Settings::default();cfg.ui_language=if state.ends_with("-en"){UiLanguage::English}else{UiLanguage::Chinese};localization::configure(cfg.ui_language,"zh-CN");
            if let Some(g)=onboarding::open(cx,tx,cfg,String::new()){let step=state.split('-').find_map(|p|p.parse::<usize>().ok()).unwrap_or(0);let _=g.update(cx,|g,_,cx|g.navigate(step,cx));}
        } else if state.starts_with("settings") {
            let mut cfg=Settings::default();cfg.ui_language=if state.ends_with("-en"){UiLanguage::English}else{UiLanguage::Chinese};localization::configure(cfg.ui_language,"zh-CN");let _=open_form(cx,tx,cfg,String::new());
        } else if state == "selection" {
            let _ = open_bee(cx, tx, selection);
        } else if let Some(handle) = open_panel(cx, tx, selection, state == "snapshot") {
            let _ = handle.update(cx, |panel, _, cx| {
                panel.body = match state.as_str() {
                    "loading" | "error" | "snapshot" => String::new(),
                    "paragraph" => "划词翻译应当响应迅速，不打扰阅读。".into(),
                    "long" => (1..=30).map(|i| format!("第{i}行：这是公开的长文本测试，验证卡片高度上限与内部滚动。\n")).collect(),
                    "partial-pron" => "读音：/ˌserən".into(),
                    "no-pron" => "- n. 意外发现美好事物的运气".into(),
                    "pron-only" => "读音：/ˌserənˈdɪpəti/\n".into(),
                    "text-prefix" | "plain-prefix" => "读音：这是普通译文的原始第一行\n正文必须完整保留。".into(),
                    "legacy" => "读音：/ˌserənˈdɪpəti/\n- n. 意外发现美好事物的运气\n- 例句：It was pure serendipity. 这纯属意外之喜。\n- 常见搭配：a moment of serendipity".into(),
                    _ => "读音：/ˌserənˈdɪpəti/\n- n. 意外发现美好事物的运气".into(),
                };
                if state == "plain-prefix" { panel.engine = Engine::LibreTranslate; }
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
#[derive(Clone, Copy, PartialEq)]
enum SaveOrigin {
    Settings,
    GuideService,
    GuidePreference,
    GuideFinish,
}
#[derive(Clone, Copy, PartialEq)]
enum RequestTarget {
    Panel,
    Guide,
}
enum Background {
    Saved(Result<Settings, String>, Option<String>, SaveOrigin),
    Request(u64, Result<stream::Request, String>, RequestTarget),
}
enum Work {
    Save(Settings, Settings, KeyEdit, SaveOrigin),
    Request(u64, Settings, String, RequestTarget),
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
    localization::configure(initial.ui_language, &super::util::system_language());
    let pool = Pool::new();
    pool.configure_cache(initial.translation_cache);
    let speech = super::speech::Speech::new();
    let (tx, rx) = mpsc::sync_channel(32);
    let (bg_tx, bg_rx) = mpsc::sync_channel(2);
    let (work_tx, work_rx) = mpsc::sync_channel(1);
    let mut cfg = initial;
    let startup_store = store.clone();
    std::thread::spawn(move || {
        while let Ok(work) = work_rx.recv() {
            let result = match work {
                Work::Save(old, next, key, origin) => {
                    let saved = {
                        let mut store = store.lock().unwrap();
                        let checked = if origin == SaveOrigin::GuideService {
                            let key = match &key {
                                KeyEdit::Replace(value) => Ok(value.trim().to_owned()),
                                KeyEdit::Keep => store.key(&next),
                                KeyEdit::Clear => Ok(String::new()),
                            };
                            key.and_then(|key| {
                                engines::validate(next.translation_engine, &next.engine_config(key))
                            })
                        } else {
                            Ok(())
                        };
                        let saved = checked.and_then(|_| store.save(&old, next, key));
                        if saved.is_ok() {
                            let _ = store.cleanup();
                        }
                        saved
                    };
                    let warning = saved.as_ref().ok().and_then(|cfg| {
                        if util::test_profile().is_some() {
                            None
                        } else {
                            super::integration::autostart(cfg.autostart).err()
                        }
                    });
                    Background::Saved(saved, warning, origin)
                }
                Work::Request(g, config, text, target) => {
                    let key = store.lock().unwrap().key(&config);
                    Background::Request(
                        g,
                        key.and_then(|key| {
                            let engine_config = config.engine_config(key.clone());
                            engines::validate(config.translation_engine, &engine_config)?;
                            Ok(stream::Request {
                                generation: g,
                                text,
                                key,
                                model: config.active_provider().model,
                                endpoint: engines::endpoint(
                                    config.translation_engine,
                                    &engine_config,
                                ),
                                engine: config.translation_engine,
                                config: engine_config,
                            })
                        }),
                        target,
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
  input::bind(cx);onboarding::bind(cx);cx.bind_keys([KeyBinding::new("tab",NextField,Some("BeeKSettings")),KeyBinding::new("shift-tab",PreviousField,Some("BeeKSettings")),KeyBinding::new("ctrl-s",SaveFields,Some("BeeKSettings")),KeyBinding::new("enter",TranslateSnapshot,Some("BeeKPreview")),KeyBinding::new("escape",CloseView,Some("BeeKPreview")),KeyBinding::new("ctrl-c",CopyPreview,Some("BeeKPreview"))]);
  let host=match cx.open_window(WindowOptions{titlebar:None,show:false,focus:false,kind:WindowKind::PopUp,window_bounds:Some(WindowBounds::Windowed(Bounds::new(point(px(0.),px(0.)),size(px(1.),px(1.))))),..Default::default()},|_,cx|cx.new(|_|Host)).map_err(|_|()){Ok(host)=>host,Err(_)=>{util::fatal("图形初始化失败，请检查显卡驱动");cx.quit();return;}};
  // A restart child reaches GPUI/GPU window creation before ChildReady. Until
  // ParentExitCommitted and actual parent death it installs no tray or hooks.
  let initialized=(||->Result<_,String>{
   let instance=if restarting{super::lifecycle::child_preflight()?;let committed=startup_settings(&mut startup_store.lock().unwrap());valid=committed.is_ok();initial_error=committed.as_ref().err().cloned().unwrap_or_default();cfg=committed.unwrap_or_default();super::lifecycle::Instance::begin(etx.clone())?.ok_or("重启后会话锁仍被占用")?}else{instance.unwrap()};
   let coordinator=Coordinator::start()?;let desktop=native::Desktop::start(etx)?;desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);Ok((instance,coordinator,desktop))
  })();
  let(mut instance,coordinator,desktop)=match initialized{Ok(v)=>v,Err(e)=>{if !restarting{util::fatal(&e);}cx.quit();return;}};
  let needs_guide=cfg.onboarding_revision<onboarding::REVISION;
  let mut guide=if needs_guide {onboarding::open(cx,tx.clone(),cfg.clone(),initial_error.clone())}else{None};
  let mut form=if needs_guide||std::env::args().any(|a|a=="--autostart"){None}else{open_form(cx,tx.clone(),cfg.clone(),initial_error.clone())};let mut bee:Option<WindowHandle<Bee>>=None;let mut panel:Option<WindowHandle<Panel>>=None;
  cx.spawn(async move|cx|{
   let(_host,_desktop)=(host,desktop);let mut config_valid=valid;let mut epoch=native::EPOCH.load(Ordering::Acquire);let mut trigger=0;let mut id=0;let mut generation=0;let mut pending:Option<(u64,Purpose)>=None;let mut cache:Option<(Selection,Instant)>=None;let mut last_source:Option<Source>=None;let mut mailbox:Option<(u64,Mailbox)>=None;let mut dragging:Option<([i32;2],[i32;2])>=None;let mut saving=false;let mut hotkey_requested=false;let mut copy_requested=false;let mut quiet_at=Instant::now();let mut system_epoch=native::SYSTEM_EPOCH.load(Ordering::Acquire);let mut edits:VecDeque<(Entity<TextInput>,ClipboardIntent)>=VecDeque::new();let mut edit_active:Option<(u64,Entity<TextInput>,ClipboardIntent)>=None;let mut failed_copy:Option<Instant>=None;let mut last_applied_edit:Option<(Entity<TextInput>,u64,u64,Instant)>=None;let mut last_settings_status:Option<String>=if initial_error.is_empty(){None}else{Some(initial_error.clone())};
   let mut guide_generation=0u64;let mut guide_pending=false;let mut guide_mailbox:Option<(u64,Mailbox)>=None;let mut settings_from_guide=false;
   let mut next_update=Instant::now()+Duration::from_secs(10);
   loop {
    if cfg.auto_update && Instant::now()>=next_update {let _=super::integration::check_updates(false);next_update=Instant::now()+Duration::from_secs(6*60*60);}
    let context=native::SYSTEM_EPOCH.load(Ordering::Acquire);if context!=system_epoch{#[cfg(feature="test-fixtures")]super::diagnostics::event("system_epoch_changed",serde_json::json!({"before":system_epoch,"after":context}));system_epoch=context;edits.clear();edit_active=None;failed_copy=None;last_applied_edit=None;pending=None;cache=None;generation+=1;pool.cancel_pending();mailbox=None;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);if let Some(f)=form{let _=f.update(cx,|f,w,cx|{let _=native::keep_accessible(w);f.status="系统状态已变化，未完成的剪贴板操作已取消".into();cx.notify();});}});}
    let changed=native::EPOCH.load(Ordering::Acquire);
    if let Some(source)=util::current(changed){last_source=Some(source);}
    if changed!=epoch{#[cfg(feature="test-fixtures")]super::diagnostics::event("ui_epoch_changed",serde_json::json!({"before":epoch,"after":changed}));epoch=changed;quiet_at=Instant::now();pending=None;cache=None;let _=cx.update(|cx|hide(&mut bee,cx));}

    if cache.as_ref().is_some_and(|(s,at)|at.elapsed()>Duration::from_millis(TTL_MS)||!util::is_current(s.source)){cache=None;let _=cx.update(|cx|hide(&mut bee,cx));}
    let t=native::triggered();if edit_active.is_none()&&edits.is_empty()&&cfg.selection_enabled&&!hotkey_requested&&!copy_requested&&pending.as_ref().is_none_or(|(_,p)|matches!(p,Purpose::Detect))&&t!=trigger&&native::quiet_since(t){trigger=t;if let Some(source)=util::current(epoch){id+=1;coordinator.request(id,source,epoch,util::cursor(),false);pending=Some((id,Purpose::Detect));}}
    while let Ok(event)=erx.try_recv(){match event{
     Event::Guide=>{let _=tx.try_send(Command::OpenGuide);},
     Event::Settings|Event::About=>{settings_from_guide=false;let about=matches!(event,Event::About);let info=format!("BeeK {} · Windows x64 · helper {} · {}",env!("CARGO_PKG_VERSION"),env!("BEEK_BUILD_ID").get(..12).unwrap_or("unknown"),coordinator.snapshot());let _=cx.update(|cx|{hide(&mut bee,cx);let active=form.and_then(|f|f.update(cx,|f,w,cx|{f.guide_mode=false;w.resize(size(px(720.),px(780.)));w.set_window_title(tr("BeeK 设置"));for field in &f.fields{field.update(cx,|field,cx|field.set_compact(false,cx));}cx.notify();native::activate(w)}).ok());if active.is_none(){form=open_form(cx,tx.clone(),cfg.clone(),if about{info.clone()}else{last_settings_status.clone().unwrap_or_default()});}if about{if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=info;cx.notify();});}}});},
     Event::OutsideClick=>{if panel.is_some_and(|p|p.update(cx,|p,_,_|!p.snapshot).unwrap_or(false)){generation+=1;pool.cancel_pending();mailbox=None;let _=cx.update(|cx|hide(&mut panel,cx));}},
     Event::Update=>{let _=tx.try_send(Command::Update);},
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
       Purpose::Preview|Purpose::Clipboard(false)=>{generation+=1;pool.cancel_pending();mailbox=None;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);panel=open_panel(cx,tx.clone(),s,true);});},
       Purpose::Clipboard(true)=>{cache=Some((s.clone(),Instant::now()));let _=cx.update(|cx|{hide(&mut bee,cx);bee=open_bee(cx,tx.clone(),s);});}
      }}
     },
     Outcome::Copied(reply,_)=>{if pending.as_ref().is_some_and(|(i,_)|*i==reply){pending=None;let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="已复制译文".into();cx.notify();});});}},
     Outcome::Empty(reply,status)=>{if pending.as_ref().is_some_and(|(i,_)|*i==reply){let purpose=pending.take().unwrap().1;let copy=matches!(purpose,Purpose::Copy);let manual=matches!(purpose,Purpose::Preview|Purpose::Clipboard(false));let _=cx.update(|cx|{hide(&mut bee,cx);if copy{if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，剪贴板可能已变化或被占用，请重试".into();cx.notify();});}}if manual{let message=selection_status(status).to_owned();if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});}else{form=open_form(cx,tx.clone(),cfg.clone(),message);}}});}},
     Outcome::Fault(reply,code)=>{let _=code;if pending.as_ref().is_some_and(|(i,_)|*i==reply){let purpose=pending.take().unwrap().1;let copy=matches!(purpose,Purpose::Copy);let manual=matches!(purpose,Purpose::Preview|Purpose::Clipboard(false));if manual{last_settings_status=Some("选区或剪贴板检测超时、繁忙或已取消，请重试".into());}let _=cx.update(|cx|{hide(&mut bee,cx);if copy{if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，请稍后重试".into();cx.notify();});}}if manual{let message=last_settings_status.clone().unwrap();if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});}else{form=open_form(cx,tx.clone(),cfg.clone(),message);}}});}},
     Outcome::Paused=>{let _=cx.update(|cx|{hide(&mut bee,cx);if let Some(f)=form{let _=f.update(cx,|s,_,cx|{s.status="选区检测已暂停，请从托盘手动恢复".into();cx.notify();});}});},_=>{}
    }}
    while let Ok(command)=rx.try_recv(){match command{
     Command::OpenGuide=>{let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);let shown=guide.and_then(|g|g.update(cx,|_,w,_|native::activate(w)).ok());if shown.is_none(){guide=onboarding::open(cx,tx.clone(),cfg.clone(),String::new());}});},
     Command::Guide(action)=>{
      use onboarding::Action;
      match action {
       Action::Close=>{guide_generation+=1;guide_pending=false;guide_mailbox=None;let _=cx.update(|cx|hide(&mut guide,cx));},
       Action::Console|Action::KeyGuide=>{let _=cx.update(|cx|cx.open_url(if matches!(action,Action::Console){"https://console.volcengine.com/ark/"}else{"https://docs.volcengine.com/docs/ark/manage-api-keys?lang=zh"}));},
       Action::Configure(engine)=>{settings_from_guide=true;let _=cx.update(|cx|{
        let existing=form.and_then(|f|f.update(cx,|f,w,cx|{f.guide_mode=true;w.resize(size(px(640.),px(650.)));w.set_window_title(tr("翻译服务设置"));for field in &f.fields{field.update(cx,|field,cx|field.set_compact(true,cx));}if let Some(engine)=engine{f.switch_engine(engine,w,cx);}if let Some(index)=f.visible_fields().first(){f.fields[*index].read(cx).focus(w);}else{w.focus(&f.focus);}let _=native::activate(w);cx.notify();}).ok());
        if existing.is_none(){let mut next=cfg.clone();if let Some(engine)=engine{next.translation_engine=engine;}form=open_form_mode(cx,tx.clone(),next,String::new(),true);}
       });},
       Action::SelectionSettings=>{settings_from_guide=false;let _=cx.update(|cx|{
        let existing=form.and_then(|f|f.update(cx,|f,w,cx|{f.guide_mode=false;w.resize(size(px(720.),px(780.)));w.set_window_title(tr("BeeK 设置"));for field in &f.fields{field.update(cx,|field,cx|field.set_compact(false,cx));}if let Some(index)=f.visible_fields().first(){f.fields[*index].read(cx).focus(w);}let _=native::activate(w);cx.notify();}).ok());
        if existing.is_none(){form=open_form(cx,tx.clone(),cfg.clone(),String::new());}
       });},
       Action::Trial=>if !guide_pending&&guide_mailbox.is_none(){
        if saving||!config_valid{let _=cx.update(|cx|if let Some(g)=guide{let _=g.update(cx,|g,_,cx|{g.notice=tr("请先填写并启用有效的翻译服务，再试译。").into();cx.notify();});});}
        else{guide_generation+=1;let g=guide_generation;guide_pending=work_tx.try_send(Work::Request(g,cfg.clone(),onboarding::sample(cfg.translation_target).into(),RequestTarget::Guide)).is_ok();let accepted=guide_pending;let _=cx.update(|cx|if let Some(g)=guide{let _=g.update(cx,|g,_,cx|{g.trial_busy=accepted;g.trial_result.clear();g.notice=if accepted{String::new()}else{tr("设置操作繁忙，请稍后重试").into()};cx.notify();});});}
       },
       Action::UiLanguage(_)|Action::Target(_)|Action::Finish|Action::Skip=>if !saving {
        let mut next=cfg.clone();let origin=match action{Action::UiLanguage(language)=>{next.ui_language=language;SaveOrigin::GuidePreference},Action::Target(target)=>{next.translation_target=target;SaveOrigin::GuidePreference},_=>{next.onboarding_revision=onboarding::REVISION;SaveOrigin::GuideFinish}};
        saving=work_tx.try_send(Work::Save(cfg.clone(),next,KeyEdit::Keep,origin)).is_ok();let accepted=saving;let _=cx.update(|cx|if let Some(g)=guide{let _=g.update(cx,|g,_,cx|{g.saving=accepted;if !accepted{g.notice=tr("设置操作繁忙，请稍后重试").into();}cx.notify();});});
       }
      }
     },
     Command::Click(s)=>if cache.as_ref().is_some_and(|(c,at)|c.same_selection(&s)&&at.elapsed()<Duration::from_millis(TTL_MS))&&s.epoch==epoch&&util::is_current(s.source){id+=1;let point=s.rects.last().map(|r|[(r.x+r.width/2.) as i32,(r.y+r.height/2.) as i32]).unwrap_or_else(util::cursor);coordinator.request(id,s.source,epoch,point,s.clipboard_sequence.is_some());pending=Some((id,Purpose::Verify(s)));},
     Command::Translate(s,snapshot)=>{
      if snapshot{let valid=panel.is_some_and(|p|p.update(cx,|p,_,_|p.snapshot&&p.selection.same_selection(&s)).unwrap_or(false));if !valid{continue;}}
      else if s.epoch!=native::EPOCH.load(Ordering::Acquire)||!util::is_current(s.source){continue;}
      generation+=1;pool.cancel_pending();let g=generation;let config=cfg.clone();let text=s.text.clone();
      let _=cx.update(|cx|{hide(&mut bee,cx);if !snapshot{hide(&mut panel,cx);panel=open_panel(cx,tx.clone(),s,false);}if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.engine=cfg.translation_engine;p.active=true;p.error=false;p.copied_until=None;p.status="正在翻译…".into();p.body.clear();cx.notify();});}});
      let error=if saving{Some("设置正在保存，请稍后重新选择或重试")}else if !config_valid{Some("配置损坏，恢复并保存设置前不能发送翻译请求")}else if work_tx.try_send(Work::Request(g,config,text,RequestTarget::Panel)).is_err(){Some("设置操作繁忙，请稍后重试")}else{None};
      if let Some(error)=error{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=false;p.error=true;p.status=error.into();cx.notify();});});}
     },
     Command::Save(next,key,service) if !saving=>{
      saving=if edit_active.is_some()||!edits.is_empty(){false}else{work_tx.try_send(Work::Save(cfg.clone(),next,key,if service{SaveOrigin::GuideService}else{SaveOrigin::Settings})).is_ok()};if !saving{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.busy=false;f.status=if edit_active.is_some()||!edits.is_empty(){"请等待剪贴板编辑完成后再保存"}else{"设置操作繁忙，请稍后重试"}.into();cx.notify();});});}
     },
     Command::Save(_,_,_)=>{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.busy=false;f.status=tr("设置正在保存，请稍后重新选择或重试").into();cx.notify();});});},
     Command::Copy(text,sequence)=>{if let Some(source)=util::source(unsafe{windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()},epoch){id+=1;coordinator.copy(id,source,epoch,text,sequence);pending=Some((id,Purpose::Copy));}else{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，请返回普通桌面后重试".into();cx.notify();});});}},
     Command::Edit(field,mut intent)=>{
      #[cfg(feature="test-fixtures")]super::diagnostics::event("editor_intent",match &intent{ClipboardIntent::Paste{ticket,..}=>serde_json::json!({"kind":"paste","ticket":ticket}),ClipboardIntent::Copy{ticket,cut,..}=>serde_json::json!({"kind":if *cut{"cut"}else{"copy"},"ticket":ticket})});if let ClipboardIntent::Paste{ticket,at,..}=&mut intent{if let Some((target,old,new,completed))=&last_applied_edit{if target==&field&&*ticket==*old&&*at<=*completed{*ticket=*new;}}}if edits.len()<4{let ignore=matches!(&intent,ClipboardIntent::Paste{at,..} if failed_copy.is_some_and(|failed|*at<=failed));if !ignore{edits.push_back((field,intent));}else{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status="先前复制未完成，关联粘贴已取消，请重试".into();cx.notify();});});}}else{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status="剪贴板操作繁忙，请稍后重试".into();cx.notify();});});}},
     Command::Speak(text)=>{let result=speech.speak(generation,text);let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status=result.map(|_|"正在朗读…").unwrap_or_else(|e|e).into();cx.notify();});});},Command::Close=>{generation+=1;pool.cancel_pending();mailbox=None;let _=cx.update(|cx|{hide(&mut panel,cx);hide(&mut bee,cx);});},
     Command::Demo=>{
      let s=Selection{source:Source{hwnd:0,pid:std::process::id(),created:1,generation:epoch},epoch,runtime_id:vec![],text:"Reading opens a window to the world.".into(),rects:vec![],clipboard_sequence:None};
      let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);panel=open_panel(cx,tx.clone(),s.clone(),true);});let _=tx.try_send(Command::Translate(s,true));
     },
     Command::Update=>{let queued=super::integration::open_updates();let _=cx.update(|cx|{if form.is_none(){form=open_form(cx,tx.clone(),cfg.clone(),String::new());}if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=if queued{"正在检查 Windows 稳定版更新…"}else{"更新检查繁忙，请稍后重试"}.into();cx.notify();});}});},
     Command::Drag(at)=>{if let Some(p)=panel{let mut origin=None;let _=p.update(cx,|p,w,_|{if !p.snapshot{if let Ok(h)=native::hwnd(w){let mut rect=windows::Win32::Foundation::RECT::default();if unsafe{windows::Win32::UI::WindowsAndMessaging::GetWindowRect(h,&mut rect)}.is_ok(){origin=Some([rect.left,rect.top]);}}}});if let Some(origin)=origin{dragging=Some((at,origin));}}},_=>{}
    }}
    if edit_active.is_none(){if let Some((field,intent))=edits.pop_front(){
     let source=form.and_then(|f|f.update(cx,|_,w,_|native::hwnd(w).ok().and_then(|h|util::source(h,epoch))).ok().flatten());
     if let Some(source)=source{id+=1;match &intent{ClipboardIntent::Paste{sequence,..}=>coordinator.paste(id,source,epoch,*sequence),ClipboardIntent::Copy{text,sequence,..}=>coordinator.copy(id,source,epoch,text.clone(),*sequence)}edit_active=Some((id,field,intent));}else{edits.clear();let message="设置窗口已不可用，未完成的输入操作已取消".to_owned();last_settings_status=Some(message.clone());let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});});}
    }}
    while let Ok(message)=bg_rx.try_recv(){match message{
     Background::Saved(result,warning,origin)=>{
      saving=false;let committed=result.is_ok();
      let status=match result{Ok(saved)=>{cfg=saved;config_valid=true;pool.configure_cache(cfg.translation_cache);localization::configure(cfg.ui_language,&super::util::system_language());generation+=1;guide_generation+=1;guide_pending=false;guide_mailbox=None;pool.cancel_pending();mailbox=None;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);});_desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);warning.unwrap_or_else(||"已保存".into())},Err(e)=>e};
      last_settings_status=Some(status.clone());let editing=edit_active.is_some()||!edits.is_empty();let mut close_service=false;
      let _=cx.update(|cx|{
       if let Some(f)=form{let _=f.update(cx,|f,_,cx|{
        if matches!(origin,SaveOrigin::Settings|SaveOrigin::GuideService){
         let signature=f.save_revision.take();let unchanged=signature.as_ref().is_some_and(|s|s==&f.signature(cx))&&!editing;f.busy=false;f.status=status.clone();
         if committed{if unchanged{f.cfg=cfg.clone();close_service=settings_from_guide;}else{f.status.push_str("；提交后的新修改尚未保存");}f.cfg.provider.credential_ref=cfg.provider.credential_ref.clone();for(name,p)in &mut f.cfg.providers{p.credential_ref=cfg.providers.get(name).and_then(|p|p.credential_ref.clone());}f.drafts.remove(cfg.translation_engine.key());f.fields[0].update(cx,|key,cx|key.set_saved_secret(f.cfg.active_provider().credential_ref.is_some(),cx));if signature.as_ref().is_some_and(|(_,flags)|flags[4]==f.clear){f.clear=false;}if !editing&&signature.as_ref().is_some_and(|(fields,_)|fields[0]==f.fields[0].read(cx).revision()){f.fields[0].update(cx,|key,cx|key.set_value("",cx));}}
        }else if committed{f.cfg.ui_language=cfg.ui_language;f.cfg.translation_target=cfg.translation_target;f.cfg.onboarding_revision=cfg.onboarding_revision;}
        cx.notify();
       });}
       if close_service{hide(&mut form,cx);settings_from_guide=false;}
       if committed&&origin==SaveOrigin::GuideFinish{hide(&mut guide,cx);}
       else if let Some(g)=guide{let _=g.update(cx,|g,w,cx|{g.saving=false;if committed{if close_service{g.settings_changed(cfg.clone(),cx);let _=native::activate(w);}else{g.cfg=cfg.clone();w.set_window_title(tr("BeeK 使用引导"));g.trial_busy=false;g.trial_result.clear();g.notice.clear();}}else{g.notice=tr(&status).into();}cx.notify();});}
      });
     },
     Background::Request(g,result,RequestTarget::Guide) if g==guide_generation=>{
      guide_pending=false;if guide.is_some_and(|w|w.update(cx,|_,_,_|()).is_ok()){
       match result {Ok(request)=>{if let Some(text)=pool.cached(&request){let _=cx.update(|cx|if let Some(g)=guide{let _=g.update(cx,|g,_,cx|g.finish_trial(Ok(text),true,cx));});}else{guide_mailbox=Some((g,pool.submit(request)));}},Err(error)=>{let _=cx.update(|cx|if let Some(g)=guide{let _=g.update(cx,|g,_,cx|g.finish_trial(Err(error),false,cx));});}}
      }
     },
     Background::Request(g,result,RequestTarget::Panel) if g==generation&&panel.is_some_and(|p|p.update(cx,|_,_,_|()).is_ok())=>match result{Ok(request)=>mailbox=Some((g,pool.submit(request))),Err(e)=>{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=false;p.error=true;p.status=e;cx.notify();});});}},_=>{}
    }}
    if let Some((g,m))=&guide_mailbox{if *g==guide_generation{let progress=std::mem::replace(&mut *m.lock().unwrap(),Progress::Idle);if let Progress::Done(result)=progress{let _=cx.update(|cx|if let Some(g)=guide{let _=g.update(cx,|g,_,cx|g.finish_trial(result,false,cx));});guide_mailbox=None;}}}
    if let Some(status)=super::integration::update_result(){last_settings_status=Some(status.clone());let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=status;cx.notify();});});}
    if !native::HOTKEY_OK.load(Ordering::Acquire){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{if !f.guide_mode&&!f.status.contains("快捷键"){f.status="全局快捷键注册失败（冲突或格式无效），请修改后保存".into();cx.notify();}});});}
    let mut completed=false;
    if let Some((g,m))=&mailbox{if *g==generation{let progress=std::mem::replace(&mut *m.lock().unwrap(),Progress::Idle);completed=matches!(progress,Progress::Done(_));if !matches!(progress,Progress::Idle){let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{match progress{Progress::Idle=>{},Progress::Loading=>{p.status="正在翻译…".into();},Progress::Queued=>{p.status="等待前面的实际请求结束…".into();},Progress::Text(text)=>{p.status="正在翻译…".into();p.body=text;},Progress::Done(Ok(text))=>{p.active=false;p.error=false;p.body=text;p.status="完成".into();},Progress::Done(Err(e))=>{p.active=false;p.error=true;p.status=e;}}cx.notify();});});}}}
    if completed{#[cfg(feature="test-fixtures")]super::diagnostics::event("translation_completed",serde_json::json!({"generation":generation}));mailbox=None;}
    if let Some((start,origin))=dragging{if unsafe{windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(1)}>=0{dragging=None;}else{let point=util::cursor();if let Some(p)=panel{let _=p.update(cx,|_,w,_|native::move_popup(w,origin[0]+point[0]-start[0],origin[1]+point[1]-start[1]));}}}
    cx.background_executor().timer(Duration::from_millis(16)).await;
   }
  }).detach();
 });
    Ok(())
}
