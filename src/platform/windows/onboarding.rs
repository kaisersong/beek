//! The macOS 960×680 onboarding composition, rendered natively by GPUI on Windows.
//! Images decode once. Navigation/demo never starts HTTP; Trial uses the existing pool.
use super::{
    native,
    ui::{Command, Tx},
};
use crate::core::{
    localization::{self, tr, UiLanguage},
    settings::Settings,
    translation_engine::Engine,
    translation_language::TargetLanguage,
};
use gpui::{prelude::*, *};
use std::sync::{Arc, OnceLock};

pub const REVISION: u16 = 2;
pub const SAMPLE: &str = "The quick brown fox jumps over the lazy dog.";
pub fn sample(target: TargetLanguage) -> &'static str {
    if target == TargetLanguage::English {
        "你好，世界。"
    } else {
        SAMPLE
    }
}
#[derive(Clone, Copy)]
pub enum Action {
    Configure(Option<Engine>),
    SelectionSettings,
    Console,
    KeyGuide,
    Trial,
    Finish,
    Skip,
    Close,
    UiLanguage(UiLanguage),
    Target(TargetLanguage),
}
actions!(onboarding, [Primary, Previous, Next, Dismiss]);
#[derive(Clone, Copy, PartialEq)]
enum Picker {
    Ui,
    Target,
}

pub struct Guide {
    pub cfg: Settings,
    pub step: usize,
    pub saving: bool,
    pub trial_busy: bool,
    pub trial_result: String,
    pub notice: String,
    picker: Option<Picker>,
    demo_revealed: bool,
    focus: FocusHandle,
    tx: Tx,
}
fn image(
    bytes: &'static [u8],
    slot: &'static OnceLock<Arc<RenderImage>>,
    cx: &App,
) -> Arc<RenderImage> {
    slot.get_or_init(|| {
        Image::from_bytes(ImageFormat::Png, bytes.to_vec())
            .to_image_data(cx.svg_renderer())
            .expect("bundled onboarding image")
    })
    .clone()
}
fn backdrop(cx: &App) -> Arc<RenderImage> {
    static IMAGE: OnceLock<Arc<RenderImage>> = OnceLock::new();
    image(
        include_bytes!("../../../resources/onboarding/horizon.png"),
        &IMAGE,
        cx,
    )
}
fn logo(cx: &App) -> Arc<RenderImage> {
    static IMAGE: OnceLock<Arc<RenderImage>> = OnceLock::new();
    image(
        include_bytes!("../../../resources/icons/BeeK-app-256.png"),
        &IMAGE,
        cx,
    )
}
#[derive(Clone, Copy)]
struct Canvas {
    s: f32,
    x: f32,
    y: f32,
}
impl Canvas {
    fn at(self, x: f32, y: f32, width: f32, height: f32) -> Div {
        div()
            .absolute()
            .left(px(self.x + x * self.s))
            .top(px(self.y + y * self.s))
            .w(px(width * self.s))
            .h(px(height * self.s))
    }
    fn label(
        self,
        text: impl Into<SharedString>,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        size: f32,
        strong: bool,
        secondary: bool,
    ) -> Div {
        self.at(x, y, width, height)
            .text_size(px(size * self.s))
            .line_height(px(size * 1.27 * self.s))
            .text_color(rgb(if secondary { 0x666b73 } else { 0x1a1c1f }))
            .when(strong, |d| d.font_weight(FontWeight::BOLD))
            .child(text.into())
    }
    fn surface(self, x: f32, y: f32, w: f32, h: f32, alpha: u32, shadow: bool) -> Div {
        self.at(x, y, w, h)
            .rounded(px(24. * self.s))
            .bg(rgba(0xffffff00 | alpha))
            .when(shadow, |d| {
                d.shadow(vec![BoxShadow {
                    color: rgba(0x20334d24).into(),
                    offset: point(px(0.), px(12. * self.s)),
                    blur_radius: px(24. * self.s),
                    spread_radius: px(0.),
                }])
            })
    }
}
impl Guide {
    pub fn navigate(&mut self, step: usize, cx: &mut Context<Self>) {
        self.step = step.min(3);
        self.picker = None;
        self.notice.clear();
        cx.notify();
    }
    pub fn settings_changed(&mut self, cfg: Settings, cx: &mut Context<Self>) {
        self.cfg = cfg;
        self.step = 3;
        self.trial_busy = false;
        self.trial_result.clear();
        self.notice = tr("服务已保存并启用，可以试译一句。").into();
        cx.notify();
    }
    pub fn finish_trial(
        &mut self,
        result: Result<String, String>,
        cached: bool,
        cx: &mut Context<Self>,
    ) {
        self.trial_busy = false;
        self.trial_result = match result {
            Ok(text) => format!(
                "{} · {}",
                tr(if cached {
                    "缓存命中"
                } else {
                    "连接成功"
                }),
                text.chars().take(150).collect::<String>()
            ),
            Err(error) => tr(&error).to_owned(),
        };
        cx.notify();
    }
    fn send(&mut self, action: Action, cx: &mut Context<Self>) {
        if self.tx.try_send(Command::Guide(action)).is_err() {
            self.notice = tr("操作队列繁忙，请稍后重试").into();
            cx.notify();
        }
    }
    fn primary(&mut self, _: &Primary, _: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        match self.step {
            0 | 1 => self.navigate(self.step + 1, cx),
            2 => self.send(Action::Configure(Some(Engine::Ark)), cx),
            _ => self.send(Action::Finish, cx),
        }
    }
    fn previous(&mut self, _: &Previous, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate(self.step.saturating_sub(1), cx);
    }
    fn next(&mut self, _: &Next, _: &mut Window, cx: &mut Context<Self>) {
        self.navigate((self.step + 1).min(3), cx);
    }
    fn dismiss(&mut self, _: &Dismiss, _: &mut Window, cx: &mut Context<Self>) {
        self.send(Action::Close, cx);
    }
    fn button(
        &self,
        c: Canvas,
        id: &'static str,
        title: &str,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        primary: bool,
        enabled: bool,
    ) -> Stateful<Div> {
        c.at(x, y, w, h)
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .gap(px(8. * c.s))
            .rounded(px(h / 2. * c.s))
            .text_size(px((if primary { 15. } else { 13. }) * c.s))
            .font_weight(if primary {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            })
            .bg(if primary {
                rgba(if enabled { 0x232627ff } else { 0x7b8087ff })
            } else {
                rgba(0xffffffad)
            })
            .text_color(if primary {
                white()
            } else {
                rgb(0x4d545c).into()
            })
            .when(!primary, |d| d.border_1().border_color(rgba(0x34495e19)))
            .when(enabled, |d| {
                d.cursor_pointer().hover(|d| {
                    d.bg(if primary {
                        rgba(0x3b4148ff)
                    } else {
                        rgba(0xffffffef)
                    })
                })
            })
            .child(tr(title).to_owned())
            .when(primary, |d| d.child("→"))
    }
    fn demo(&self, c: Canvas, interactive: bool, cx: &mut Context<Self>) -> Div {
        let mut scene = div();
        scene = scene.child(
            c.at(445., 283., 490., 47.)
                .flex()
                .items_center()
                .font_family("Georgia")
                .text_size(px(26. * c.s))
                .text_color(rgb(0x171b1e))
                .child("A little ")
                .child(div().bg(rgba(0xffdc6ea6)).child("curiosity"))
                .child(" changes everything."),
        );
        scene = scene.child(
            c.at(624., 323., 44., 44.)
                .id("onboarding-demo-bee")
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(rgba(0xfffffff5))
                .shadow_md()
                .cursor_pointer()
                .child(img(logo(cx)).size(px(28. * c.s)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, cx| {
                        s.demo_revealed = true;
                        cx.notify();
                    }),
                ),
        );
        if !interactive || self.demo_revealed {
            scene = scene
                .child(c.surface(464., 370., 340., 114., 230, true))
                .child(c.label(
                    tr("一点好奇心，改变一切。").to_owned(),
                    487.,
                    390.,
                    294.,
                    35.,
                    25.,
                    false,
                    false,
                ))
                .child(c.at(488., 441.5, 292., 0.5).bg(rgba(0x4d66801f)))
                .child(c.at(488., 449., 20., 20.).child(img(logo(cx)).size_full()))
                .child(c.label("BeeK", 517., 447., 100., 25., 15., false, true));
        } else {
            scene = scene.child(c.label(
                tr("点一下小蜜蜂，试试看。").to_owned(),
                515.,
                411.,
                330.,
                26.,
                16.,
                false,
                true,
            ));
        }
        scene
    }
    fn picker(&self, c: Canvas, kind: Picker, cx: &mut Context<Self>) -> Stateful<Div> {
        let (x, y, w, label) = match kind {
            Picker::Ui => (747., 25., 165., tr(self.cfg.ui_language.label())),
            Picker::Target => (580., 179., 204., tr(self.cfg.translation_target.label())),
        };
        c.at(x, y, w, 30.)
            .id(if kind == Picker::Ui {
                "onboarding-ui-language"
            } else {
                "onboarding-target"
            })
            .flex()
            .items_center()
            .justify_between()
            .px(px(10. * c.s))
            .rounded(px(6. * c.s))
            .bg(rgba(0x698dab25))
            .text_size(px(14. * c.s))
            .cursor_pointer()
            .child(label.to_owned())
            .child("⌄")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |s, _, _, cx| {
                    cx.stop_propagation();
                    s.picker = if s.picker == Some(kind) {
                        None
                    } else {
                        Some(kind)
                    };
                    cx.notify();
                }),
            )
    }
}
impl Render for Guide {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let width = f32::from(viewport.width);
        let height = f32::from(viewport.height);
        let scale = (width / 960.).min(height / 680.);
        let c = Canvas {
            s: scale,
            x: (width - 960. * scale) / 2.,
            y: (height - 680. * scale) / 2.,
        };
        let (title, body) = match self.step {
            0 => (
                "世界很大，\n理解很轻。",
                "选中文字，点一下 BeeK。\n让翻译自然地融入你的工作。",
            ),
            1 => (
                "轻轻一选，\n译文就在身边。",
                "在任何应用里选中文字，\n点击选区旁的小蜜蜂，即可阅读译文。",
            ),
            2 => (
                "让豆包，\n成为翻译搭档。",
                "推荐使用豆包模型。\n流式翻译、单词详解，理解更进一步。",
            ),
            _ => (
                "准备好了，\n去探索吧。",
                "点击右侧的「点击试译」按钮。\n无需选中英文，即可验证翻译服务。",
            ),
        };
        let mut view = div()
            .id("beek-onboarding")
            .relative()
            .size_full()
            .overflow_hidden()
            .font_family("Segoe UI")
            .bg(white())
            .text_color(rgb(0x1a1c1f))
            .key_context("BeeKOnboarding")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::primary))
            .on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::next))
            .on_action(cx.listener(Self::dismiss))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, _, _, cx| {
                    if s.picker.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(
                img(backdrop(cx))
                    .absolute()
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .child(
                c.at(0., 0., 730., 58.)
                    .id("onboarding-drag")
                    .on_mouse_down(MouseButton::Left, |_, w, _| w.start_window_move()),
            )
            .child(c.at(52., 85., 50., 50.).child(img(logo(cx)).size_full()))
            .child(c.label("BeeK", 116., 87., 170., 51., 31., true, false))
            .child(c.label(
                tr(title).to_owned(),
                50.,
                209.,
                385.,
                145.,
                if localization::is_english() { 44. } else { 48. },
                true,
                false,
            ))
            .child(c.label(tr(body).to_owned(), 52., 361., 370., 70., 18., false, true));
        match self.step {
            0 => view = view.child(self.demo(c, false, cx)),
            1 => {
                view = view
                    .child(self.demo(c, true, cx))
                    .child(c.label(
                        tr("Windows 无需开启辅助功能权限。").to_owned(),
                        52.,
                        451.,
                        370.,
                        30.,
                        13.,
                        false,
                        true,
                    ))
                    .child(
                        self.button(
                            c,
                            "onboarding-selection-settings",
                            "查看取词设置",
                            43.,
                            490.,
                            208.,
                            35.,
                            false,
                            true,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|s, _, _, cx| s.send(Action::SelectionSettings, cx)),
                        ),
                    )
                    .child(c.label(
                        tr("演示只在本机完成，不调用翻译服务。").to_owned(),
                        480.,
                        518.,
                        410.,
                        25.,
                        12.,
                        false,
                        true,
                    ));
            }
            2 => {
                view = view
                    .child(
                        c.label(
                            tr("调用使用你的火山方舟账号额度。\n也可以先体验无需 Key 的免费百度。")
                                .to_owned(),
                            52.,
                            441.,
                            367.,
                            47.,
                            13.,
                            false,
                            true,
                        ),
                    )
                    .child(c.surface(461., 143., 440., 332., 204, false))
                    .child(c.label(
                        tr("连接豆包，只需三步").to_owned(),
                        487.,
                        169.,
                        390.,
                        30.,
                        21.,
                        true,
                        false,
                    ));
                for (index, (heading, detail)) in [
                    ("创建 API Key", "在火山方舟控制台创建你的密钥。"),
                    ("选择豆包模型", "开通模型，复制 Model ID 或接入点 ID。"),
                    ("填入 BeeK，保存并启用", "模型可留空，使用默认豆包模型。"),
                ]
                .into_iter()
                .enumerate()
                {
                    let y = 233. + index as f32 * 76.;
                    view = view
                        .child(c.label(
                            format!("0{}", index + 1),
                            487.,
                            y,
                            35.,
                            27.,
                            13.,
                            true,
                            true,
                        ))
                        .child(c.label(
                            tr(heading).to_owned(),
                            534.,
                            y,
                            338.,
                            29.,
                            16.,
                            true,
                            false,
                        ))
                        .child(c.label(
                            tr(detail).to_owned(),
                            534.,
                            y + 24.,
                            338.,
                            25.,
                            13.,
                            false,
                            true,
                        ));
                }
                view = view
                    .child(
                        self.button(
                            c,
                            "onboarding-console",
                            "打开火山方舟",
                            471.,
                            488.,
                            171.,
                            34.,
                            false,
                            true,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|s, _, _, cx| s.send(Action::Console, cx)),
                        ),
                    )
                    .child(
                        self.button(
                            c,
                            "onboarding-key-guide",
                            "API Key 官方说明",
                            675.,
                            488.,
                            195.,
                            34.,
                            false,
                            true,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|s, _, _, cx| s.send(Action::KeyGuide, cx)),
                        ),
                    )
                    .child(c.label(
                        format!(
                            "{}{}",
                            tr("当前服务："),
                            tr(self.cfg.translation_engine.label())
                        ),
                        477.,
                        548.,
                        422.,
                        26.,
                        12.,
                        false,
                        true,
                    ))
                    .child(
                        self.button(
                            c,
                            "onboarding-other-service",
                            "切换其他翻译服务…",
                            472.,
                            580.,
                            230.,
                            34.,
                            false,
                            true,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|s, _, _, cx| s.send(Action::Configure(None), cx)),
                        ),
                    );
            }
            _ => {
                let status = if self.cfg.selection_enabled {
                    "选区检测已开启，可以在其他应用中取词。"
                } else {
                    "可在设置中开启划词取词。"
                };
                view = view
                    .child(c.label(
                        tr(status).to_owned(),
                        52.,
                        442.,
                        366.,
                        34.,
                        13.,
                        false,
                        true,
                    ))
                    .child(
                        self.button(
                            c,
                            "onboarding-selection-settings",
                            "查看取词设置",
                            43.,
                            482.,
                            211.,
                            34.,
                            false,
                            true,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|s, _, _, cx| s.send(Action::SelectionSettings, cx)),
                        ),
                    )
                    .child(c.surface(461., 106., 440., 400., 230, true))
                    .child(c.label(
                        tr("体验你的第一句翻译").to_owned(),
                        486.,
                        133.,
                        390.,
                        33.,
                        23.,
                        true,
                        false,
                    ))
                    .child(c.label(
                        tr("翻译为").to_owned(),
                        487.,
                        181.,
                        94.,
                        24.,
                        14.,
                        false,
                        true,
                    ))
                    .child(c.label(
                        format!(
                            "{}{}",
                            tr("当前服务："),
                            tr(self.cfg.translation_engine.label())
                        ),
                        487.,
                        214.,
                        389.,
                        32.,
                        12.,
                        false,
                        true,
                    ))
                    .child(c.label(
                        sample(self.cfg.translation_target),
                        487.,
                        247.,
                        389.,
                        56.,
                        21.,
                        false,
                        false,
                    ))
                    .child(c.label(
                        tr("无需选中英文，点击下方按钮即可。").to_owned(),
                        487.,
                        312.,
                        389.,
                        24.,
                        13.,
                        true,
                        true,
                    ))
                    .child(c.label(
                        if self.trial_busy {
                            tr("正在测试，请稍候…").to_owned()
                        } else if self.trial_result.is_empty() {
                            tr("点击按钮后，才会发送这句公开例句。\n重复试译优先使用本机缓存。")
                                .to_owned()
                        } else {
                            self.trial_result.clone()
                        },
                        487.,
                        346.,
                        389.,
                        75.,
                        14.,
                        false,
                        true,
                    ))
                    .child(
                        self.button(
                            c,
                            "onboarding-trial",
                            if self.trial_busy {
                                "正在翻译…"
                            } else {
                                "点击试译"
                            },
                            487.,
                            438.,
                            190.,
                            44.,
                            true,
                            !self.trial_busy && !self.saving,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|s, _, _, cx| {
                                if !s.trial_busy && !s.saving {
                                    s.send(Action::Trial, cx);
                                }
                            }),
                        ),
                    )
                    .child(
                        self.button(
                            c,
                            "onboarding-other-service",
                            "更换翻译服务…",
                            469.,
                            508.,
                            213.,
                            34.,
                            false,
                            true,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|s, _, _, cx| s.send(Action::Configure(None), cx)),
                        ),
                    )
                    .child(self.picker(c, Picker::Target, cx));
            }
        }
        if !self.notice.is_empty() {
            view = view.child(c.label(self.notice.clone(), 52., 159., 368., 38., 12., false, true));
        }
        let next = match self.step {
            0 => "开始体验",
            1 => "选择翻译服务",
            2 => "连接豆包…",
            _ => "开始使用 BeeK",
        };
        view = view.child(
            self.button(
                c,
                "onboarding-next",
                next,
                50.,
                540.,
                260.,
                54.,
                true,
                !self.saving,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, _, w, cx| s.primary(&Primary, w, cx)),
            ),
        );
        for step in 0..4 {
            view = view.child(
                c.at(52. + step as f32 * 38., 630., 30., 6.)
                    .id(SharedString::from(format!("onboarding-step-{step}")))
                    .rounded_full()
                    .bg(if self.step == step {
                        rgba(0x26292bff)
                    } else {
                        rgba(0x20262e21)
                    })
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |s, _, _, cx| s.navigate(step, cx)),
                    ),
            );
        }
        view = view.child(
            self.button(
                c,
                "onboarding-skip",
                if self.step == 2 {
                    "先用当前服务"
                } else {
                    "稍后设置"
                },
                252.,
                620.,
                if self.step == 2 { 160. } else { 124. },
                28.,
                false,
                !self.saving,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, _, _, cx| {
                    if !s.saving {
                        if s.step == 2 {
                            s.navigate(3, cx);
                        } else {
                            s.send(Action::Skip, cx);
                        }
                    }
                }),
            ),
        );
        if self.step > 0 {
            view = view.child(
                self.button(
                    c,
                    "onboarding-back",
                    "上一步",
                    824.,
                    72.,
                    88.,
                    30.,
                    false,
                    true,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, cx| s.navigate(s.step.saturating_sub(1), cx)),
                ),
            );
        }
        view = view.child(self.picker(c, Picker::Ui, cx)).child(
            c.at(925., 7., 28., 25.)
                .id("onboarding-close")
                .rounded(px(5. * c.s))
                .text_size(px(20. * c.s))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|d| d.bg(rgb(0xc84242)).text_color(white()))
                .child("×")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, cx| s.send(Action::Close, cx)),
                ),
        );
        if let Some(kind) = self.picker {
            let (x, y, w, choices): (f32, f32, f32, Vec<(String, Action)>) = match kind {
                Picker::Ui => (
                    747.,
                    58.,
                    165.,
                    UiLanguage::ALL
                        .into_iter()
                        .map(|l| (tr(l.label()).to_owned(), Action::UiLanguage(l)))
                        .collect(),
                ),
                Picker::Target => (
                    580.,
                    213.,
                    204.,
                    TargetLanguage::ALL
                        .into_iter()
                        .map(|l| (tr(l.label()).to_owned(), Action::Target(l)))
                        .collect(),
                ),
            };
            let mut menu = c
                .at(x, y, w, choices.len() as f32 * 32. + 8.)
                .id("onboarding-picker-menu")
                .occlude()
                .bg(white())
                .border_1()
                .border_color(rgba(0x20262e1c))
                .rounded(px(8. * c.s))
                .shadow_lg()
                .p(px(4. * c.s))
                .flex()
                .flex_col();
            for (index, (label, action)) in choices.into_iter().enumerate() {
                menu = menu.child(
                    div()
                        .id(index)
                        .h(px(32. * c.s))
                        .px(px(8. * c.s))
                        .flex()
                        .items_center()
                        .text_size(px(13. * c.s))
                        .rounded(px(4. * c.s))
                        .cursor_pointer()
                        .hover(|d| d.bg(rgb(0xeaf0f6)))
                        .child(label)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |s, _, _, cx| {
                                cx.stop_propagation();
                                s.picker = None;
                                s.send(action, cx);
                                cx.notify();
                            }),
                        ),
                );
            }
            view = view.child(menu);
        }
        view
    }
}

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("enter", Primary, Some("BeeKOnboarding")),
        KeyBinding::new("left", Previous, Some("BeeKOnboarding")),
        KeyBinding::new("right", Next, Some("BeeKOnboarding")),
        KeyBinding::new("escape", Dismiss, Some("BeeKOnboarding")),
    ]);
}
pub fn open(cx: &mut App, tx: Tx, cfg: Settings, notice: String) -> Option<WindowHandle<Guide>> {
    let fit = cx
        .primary_display()
        .map(|d| {
            let b = d.bounds();
            ((f32::from(b.size.width) - 40.) / 960.)
                .min((f32::from(b.size.height) - 64.) / 680.)
                .clamp(0.5, 1.)
        })
        .unwrap_or(1.);
    let handle = cx
        .open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(tr("BeeK 使用引导").into()),
                    appears_transparent: true,
                    ..Default::default()
                }),
                is_resizable: false,
                window_background: WindowBackgroundAppearance::Opaque,
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(960. * fit), px(680. * fit)),
                    cx,
                ))),
                ..Default::default()
            },
            |w, cx| {
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    w.focus(&focus);
                    Guide {
                        cfg,
                        step: 0,
                        saving: false,
                        trial_busy: false,
                        trial_result: String::new(),
                        notice,
                        picker: None,
                        demo_revealed: false,
                        focus,
                        tx,
                    }
                })
            },
        )
        .ok()?;
    let _ = handle.update(cx, |_, w, _| {
        let _ = native::keep_accessible(w);
        let _ = native::activate(w);
    });
    Some(handle)
}
