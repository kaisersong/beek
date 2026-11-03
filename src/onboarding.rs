//! Main-thread native onboarding. Only explicit trial clicks start a network request.
use crate::config::TargetLanguage;
use beek::core::localization::{self, UiLanguage};
use cocoa::base::{id, nil};
use cocoa::foundation::{NSInteger, NSPoint, NSRect, NSSize, NSString};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel, NO, YES};
use objc::{class, msg_send, sel, sel_impl};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

pub const REVISION: u16 = 1;
pub const SAMPLE: &str = "The quick brown fox jumps over the lazy dog.";
fn sample(target: TargetLanguage) -> &'static str {
    if target == TargetLanguage::English {
        "你好，世界。"
    } else {
        SAMPLE
    }
}
const CONSOLE: &str = "https://console.volcengine.com/ark/";
const KEY_GUIDE: &str = "https://docs.volcengine.com/docs/ark/manage-api-keys?lang=zh";
static SENDER: OnceLock<mpsc::Sender<crate::UiEvent>> = OnceLock::new();
static TEST_BUSY: AtomicBool = AtomicBool::new(false);
static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
enum TrialResult {
    Pending,
    Complete { cached: bool, text: String },
    Failed(String),
}

#[derive(Clone, Copy, Debug)]
#[repr(i64)]
pub enum Action {
    Welcome,
    Services,
    Ready,
    Next,
    Back,
    Configure,
    Other,
    Console,
    Guide,
    Permissions,
    Trial,
    Finish,
    Skip,
    Closed,
    Usage,
    Demo,
}

pub fn should_show(revision: u16, diagnostic: bool) -> bool {
    revision < REVISION && !diagnostic
}

extern "C" fn action(_this: &Object, _sel: Sel, sender: id) {
    let tag: NSInteger = unsafe { msg_send![sender, tag] };
    let action = match tag {
        0 => Action::Welcome,
        1 => Action::Services,
        2 => Action::Ready,
        3 => Action::Next,
        4 => Action::Back,
        5 => Action::Configure,
        6 => Action::Other,
        7 => Action::Console,
        8 => Action::Guide,
        9 => Action::Permissions,
        10 => Action::Trial,
        11 => Action::Finish,
        12 => Action::Skip,
        14 => Action::Usage,
        15 => Action::Demo,
        _ => return,
    };
    if let Some(tx) = SENDER.get() {
        let _ = tx.send(crate::UiEvent::OnboardingAction(action));
    }
}
extern "C" fn closed(_this: &Object, _sel: Sel, _notification: id) {
    if let Some(tx) = SENDER.get() {
        let _ = tx.send(crate::UiEvent::OnboardingAction(Action::Closed));
    }
}
extern "C" fn language_changed(_this: &Object, _sel: Sel, sender: id) {
    let (index, kind): (usize, i64) = unsafe {
        (
            msg_send![sender, indexOfSelectedItem],
            msg_send![sender, tag],
        )
    };
    if let Some(tx) = SENDER.get() {
        let event = if kind == 1 {
            UiLanguage::ALL
                .get(index)
                .copied()
                .map(crate::UiEvent::SetUiLanguage)
        } else {
            TargetLanguage::ALL
                .get(index)
                .copied()
                .map(crate::UiEvent::SetTranslationTarget)
        };
        if let Some(event) = event {
            let _ = tx.send(event);
        }
    }
}
fn delegate_class() -> &'static Class {
    static CLASS: OnceLock<&'static Class> = OnceLock::new();
    CLASS.get_or_init(|| {
        let mut decl = ClassDecl::new("BeeKOnboardingDelegate", class!(NSObject)).unwrap();
        unsafe {
            decl.add_method(
                sel!(onboardingAction:),
                action as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowWillClose:),
                closed as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(languageSelectionChanged:),
                language_changed as extern "C" fn(&Object, Sel, id),
            );
        }
        decl.register()
    })
}
unsafe fn string(value: &str) -> id {
    let value = NSString::alloc(nil).init_str(localization::tr(value));
    let _: id = msg_send![value, autorelease];
    value
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

// Parent retains each child. Drop the local +1 immediately; replacing the root
// content view then releases its entire previous subtree in one ownership chain.
unsafe fn attach(parent: id, child: id) {
    let _: () = msg_send![parent,addSubview:child];
    let _: () = msg_send![child, release];
}
unsafe fn color(r: f64, g: f64, b: f64, alpha: f64) -> id {
    msg_send![class!(NSColor), colorWithSRGBRed:r green:g blue:b alpha:alpha]
}
unsafe fn label(
    parent: id,
    text: &str,
    frame: NSRect,
    size: f64,
    strong: bool,
    secondary: bool,
) -> id {
    let text = localization::tr(text);
    let view: id = msg_send![class!(NSTextField), alloc];
    let view: id = msg_send![view,initWithFrame:frame];
    let _: () = msg_send![view,setStringValue:string(text)];
    let _: () = msg_send![view,setEditable:NO];
    let _: () = msg_send![view,setSelectable:NO];
    let _: () = msg_send![view,setBordered:NO];
    let _: () = msg_send![view,setDrawsBackground:NO];
    let font: id =
        msg_send![class!(NSFont),systemFontOfSize:size weight:if strong {0.4f64}else{0.0f64}];
    let _: () = msg_send![view,setFont:font];
    let ink = if secondary {
        color(0.40, 0.42, 0.45, 1.)
    } else {
        color(0.10, 0.11, 0.12, 1.)
    };
    let _: () = msg_send![view,setTextColor:ink];
    let cell: id = msg_send![view, cell];
    let _: () = msg_send![cell,setWraps:YES];
    let _: () = msg_send![cell,setScrollable:NO];
    let _: () = msg_send![view,setMaximumNumberOfLines:0i64];
    attach(parent, view);
    view
}
unsafe fn surface(parent: id, frame: NSRect, radius: f64, alpha: f64, shadow: bool) -> id {
    let view: id = msg_send![class!(NSView), alloc];
    let view: id = msg_send![view,initWithFrame:frame];
    let _: () = msg_send![view,setWantsLayer:YES];
    let layer: id = msg_send![view, layer];
    let _: () = msg_send![layer,setMasksToBounds:NO];
    let fill: id = msg_send![color(1., 1., 1., alpha), CGColor];
    let _: () = msg_send![layer,setBackgroundColor:fill];
    let _: () = msg_send![layer,setCornerRadius:radius];
    if shadow {
        let shade: id = msg_send![color(0.12, 0.20, 0.28, 1.), CGColor];
        let _: () = msg_send![layer,setShadowColor:shade];
        let _: () = msg_send![layer,setShadowOpacity:0.14f32];
        let _: () = msg_send![layer,setShadowRadius:24f64];
        let _: () = msg_send![layer,setShadowOffset:NSSize::new(0.,-12.)];
        let border: id = msg_send![color(1., 1., 1., 0.85), CGColor];
        let _: () = msg_send![layer,setBorderColor:border];
        let _: () = msg_send![layer,setBorderWidth:1f64];
    }
    attach(parent, view);
    view
}
unsafe fn picture(parent: id, image: id, frame: NSRect) {
    let view: id = msg_send![class!(NSImageView), alloc];
    let view: id = msg_send![view,initWithFrame:frame];
    let _: () = msg_send![view,setImage:image];
    let _: () = msg_send![view,setImageScaling:3u64];
    attach(parent, view);
}
// NSData is autoreleased; each decoded image is +1 and owned by OnboardingWindow.
// Views retain that image. It is decoded once per window, never on each refresh.
unsafe fn load_image(bytes: &[u8]) -> id {
    let data: id = msg_send![class!(NSData),dataWithBytes:bytes.as_ptr() length:bytes.len()];
    let image: id = msg_send![class!(NSImage), alloc];
    msg_send![image,initWithData:data]
}
unsafe fn button(
    parent: id,
    delegate: id,
    title: &str,
    frame: NSRect,
    a: Action,
    primary: bool,
    enabled: bool,
    identifier: &str,
) -> id {
    let title = localization::tr(title);
    let view: id = msg_send![class!(NSButton), alloc];
    let view: id = msg_send![view,initWithFrame:frame];
    let _: () = msg_send![view,setTitle:string(title)];
    let _: () = msg_send![view,setButtonType:0u64];
    let _: () = msg_send![view,setBordered:NO];
    let _: () = msg_send![view,setWantsLayer:YES];
    let font: id = msg_send![class!(NSFont),systemFontOfSize:if !primary && matches!(a,Action::Back|Action::Skip|Action::Next){13f64}else{15f64} weight:if primary {0.23f64}else{0.0f64}];
    let ink = if primary {
        color(1., 1., 1., 1.)
    } else {
        color(0.30, 0.33, 0.36, 1.)
    };
    let keys = [string("NSFont"), string("NSColor")];
    let values = [font, ink];
    let attributes: id = msg_send![class!(NSDictionary),dictionaryWithObjects:values.as_ptr() forKeys:keys.as_ptr() count:2usize];
    let title_string: id = msg_send![class!(NSAttributedString), alloc];
    let title_string: id =
        msg_send![title_string,initWithString:string(title) attributes:attributes];
    let _: () = msg_send![view,setAttributedTitle:title_string];
    let _: () = msg_send![title_string, release];
    let layer: id = msg_send![view, layer];
    let _: () = msg_send![layer,setCornerRadius:frame.size.height/2.];
    if primary {
        let fill: id = msg_send![color(0.13, 0.14, 0.15, 1.), CGColor];
        let _: () = msg_send![layer,setBackgroundColor:fill];
        let arrow: id = msg_send![class!(NSImage),imageWithSystemSymbolName:string("arrow.right") accessibilityDescription:nil];
        let small: id = msg_send![arrow, copy];
        let _: () = msg_send![small,setSize:NSSize::new(14.,14.)];
        let _: () = msg_send![view,setImage:small];
        let _: () = msg_send![small, release];
        let _: () = msg_send![view,setImagePosition:3u64];
        let _: () = msg_send![view,setImageScaling:2u64];
        let _: () = msg_send![view,setImageHugsTitle:YES];
        let _: () = msg_send![view,setContentTintColor:color(1.,1.,1.,1.)];
    } else if matches!(a, Action::Back | Action::Skip | Action::Next) {
        let fill: id = msg_send![color(1., 1., 1., 0.68), CGColor];
        let border: id = msg_send![color(0.2, 0.3, 0.4, 0.10), CGColor];
        let _: () = msg_send![layer,setBackgroundColor:fill];
        let _: () = msg_send![layer,setBorderColor:border];
        let _: () = msg_send![layer,setBorderWidth:0.5f64];
    }
    let _: () = msg_send![view,setTarget:delegate];
    let _: () = msg_send![view,setAction:sel!(onboardingAction:)];
    let _: () = msg_send![view,setTag:a as i64];
    let _: () = msg_send![view,setIdentifier:string(identifier)];
    let _: () = msg_send![view,setAccessibilityLabel:string(title)];
    let _: () = msg_send![view,setEnabled:if enabled {YES}else{NO}];
    if primary && matches!(a, Action::Next | Action::Finish | Action::Configure) {
        let _: () = msg_send![view,setKeyEquivalent:string("\r")];
    }
    attach(parent, view);
    view
}
unsafe fn language_picker(
    parent: id,
    delegate: id,
    frame: NSRect,
    items: &[&str],
    selected: usize,
    kind: i64,
    identifier: &str,
) {
    let view: id = msg_send![class!(NSPopUpButton), alloc];
    let view: id = msg_send![view,initWithFrame:frame pullsDown:NO];
    for item in items {
        let _: () = msg_send![view,addItemWithTitle:string(localization::tr(item))];
    }
    let _: () = msg_send![view,selectItemAtIndex:selected];
    let _: () = msg_send![view,setTarget:delegate];
    let _: () = msg_send![view,setAction:sel!(languageSelectionChanged:)];
    let _: () = msg_send![view,setTag:kind];
    let _: () = msg_send![view,setIdentifier:string(identifier)];
    let _: () = msg_send![view,setAccessibilityLabel:string(localization::tr(if kind==1 {"界面语言"}else{"翻译为"}))];
    attach(parent, view);
}

pub struct OnboardingWindow {
    window: id,
    delegate: id,
    tx: mpsc::Sender<crate::UiEvent>,
    pub step: usize,
    width: f64,
    height: f64,
    backdrop: id,
    icon: id,
    demo_revealed: bool,
    rendered_step: Option<usize>,
    notice: String,
    trial_result: Option<TrialResult>,
    pending: Option<u64>,
    last_refresh: Instant,
    signature: (bool, bool, String),
    _main_thread: PhantomData<Rc<()>>,
}
impl OnboardingWindow {
    pub fn new(tx: mpsc::Sender<crate::UiEvent>) -> Self {
        let _pool = crate::macos_runtime::AutoreleasePool::new();
        let _ = SENDER.set(tx.clone());
        unsafe {
            let screen: id = msg_send![class!(NSScreen), mainScreen];
            let available: NSRect = msg_send![screen, visibleFrame];
            let scale = ((available.size.width - 40.) / 960.)
                .min((available.size.height - 64.) / 680.)
                .clamp(0.5, 1.);
            let width = 960. * scale;
            let height = 680. * scale;
            let window: id = msg_send![class!(NSWindow), alloc];
            let window: id = msg_send![window,initWithContentRect:rect(available.origin.x+(available.size.width-width)/2.,available.origin.y+(available.size.height-height)/2.,width,height) styleMask:(3u64 | (1u64 << 15)) backing:2u64 defer:NO];
            assert_ne!(window, nil);
            let _: () = msg_send![window,setTitle:string("BeeK 使用引导")];
            let _: () = msg_send![window,setReleasedWhenClosed:NO];
            let _: () = msg_send![window,setRestorable:NO];
            let _: () = msg_send![window,setTitlebarAppearsTransparent:YES];
            let _: () = msg_send![window,setTitleVisibility:1i64];
            let _: () = msg_send![window,setBackgroundColor:color(1.,1.,1.,1.)];
            let appearance: id =
                msg_send![class!(NSAppearance),appearanceNamed:string("NSAppearanceNameAqua")];
            let _: () = msg_send![window,setAppearance:appearance];
            let delegate: id = msg_send![delegate_class(), new];
            let _: () = msg_send![window,setDelegate:delegate];
            let backdrop = load_image(include_bytes!("../resources/onboarding/horizon.png"));
            let icon = load_image(include_bytes!("../resources/icons/BeeK-app-256.png"));
            assert_ne!(backdrop, nil);
            assert_ne!(icon, nil);
            let mut this = Self {
                window,
                delegate,
                tx,
                step: 0,
                width,
                height,
                backdrop,
                icon,
                demo_revealed: false,
                rendered_step: None,
                notice: String::new(),
                trial_result: None,
                pending: None,
                last_refresh: Instant::now(),
                signature: (false, false, String::new()),
                _main_thread: PhantomData,
            };
            this.render();
            this.show();
            this
        }
    }
    pub fn show(&self) {
        let _pool = crate::macos_runtime::AutoreleasePool::new();
        crate::ax::pulse_self_ui();
        unsafe {
            let app: id = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app,setActivationPolicy:0i64];
            let _: () = msg_send![self.window,makeKeyAndOrderFront:nil];
            let _: () = msg_send![app,activateIgnoringOtherApps:YES];
        }
    }
    pub fn navigate(&mut self, action: Action) {
        self.step = match action {
            Action::Welcome => 0,
            Action::Usage => 1,
            Action::Services => 2,
            Action::Ready => 3,
            Action::Next => (self.step + 1).min(3),
            Action::Back => self.step.saturating_sub(1),
            _ => self.step,
        };
        self.notice.clear();
        self.render();
    }
    pub fn settings_changed(&mut self) {
        self.pending = None;
        self.trial_result = None;
        self.notice = "服务已保存并启用，可以试译一句。".into();
        self.step = 3;
        self.render();
    }
    pub fn demo(&mut self) {
        self.demo_revealed = true;
        self.render();
    }
    pub fn target_changed(&mut self) {
        self.pending = None;
        self.trial_result = None;
        self.notice.clear();
        self.render();
    }
    pub fn presentation_changed(&mut self) {
        self.render();
    }
    fn trial_message(&self) -> String {
        match &self.trial_result {
            None => {
                localization::tr("点击按钮后，才会发送这句公开例句。\n重复试译优先使用本机缓存。")
                    .into()
            }
            Some(TrialResult::Pending) => localization::tr("正在测试，请稍候…").into(),
            Some(TrialResult::Complete { cached, text }) => format!(
                "{} · {}",
                localization::tr(if *cached {
                    "缓存命中"
                } else {
                    "连接成功"
                }),
                text
            ),
            Some(TrialResult::Failed(message)) => localization::tr(message).into(),
        }
    }
    pub fn message(&mut self, message: String) {
        self.notice = message;
        self.render();
    }
    pub fn finish_trial(&mut self, id: u64, result: Result<String, String>) {
        if self.pending != Some(id) {
            return;
        }
        self.pending = None;
        self.trial_result = Some(match result {
            Ok(text) => TrialResult::Complete {
                cached: false,
                text: text.chars().take(150).collect(),
            },
            Err(error) => TrialResult::Failed(if error.contains("401") || error.contains("403") {
                "鉴权未通过，请检查 API Key 及其模型访问权限。".into()
            } else if error.contains("429") {
                "账号额度或请求频率受限，请检查服务控制台。".into()
            } else {
                "测试未成功，请检查模型 ID、服务地址、账号额度与网络后重试。".into()
            }),
        });
        self.render();
    }
    pub fn trial(&mut self) {
        if TEST_BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            self.message("上一次测试尚未结束，请稍候。".into());
            return;
        }
        let config = crate::config::load();
        let request = match crate::translate::TranslationRequest::from_config(
            &config,
            sample(config.translation_target),
            false,
        ) {
            Ok(r) => r,
            Err(_) => {
                TEST_BUSY.store(false, Ordering::SeqCst);
                self.message("请先填写并启用有效的翻译服务，再试译。".into());
                return;
            }
        };
        if let Some(text) = request.cached() {
            TEST_BUSY.store(false, Ordering::SeqCst);
            self.pending = None;
            self.trial_result = Some(TrialResult::Complete {
                cached: true,
                text: text.chars().take(150).collect(),
            });
            self.render();
            return;
        }
        let id = TEST_SEQUENCE.fetch_add(1, Ordering::SeqCst);
        self.pending = Some(id);
        self.trial_result = Some(TrialResult::Pending);
        self.render();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            struct BusyGuard;
            impl Drop for BusyGuard {
                fn drop(&mut self) {
                    TEST_BUSY.store(false, Ordering::SeqCst);
                }
            }
            let guard = BusyGuard;
            let result = request.stream(|_| {});
            drop(guard);
            let _ = tx.send(crate::UiEvent::OnboardingTrialDone(id, result));
        });
    }
    pub fn open_console() {
        crate::update::open_url(CONSOLE);
    }
    pub fn open_guide() {
        crate::update::open_url(KEY_GUIDE);
    }
    pub fn refresh(&mut self) {
        if self.last_refresh.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_refresh = Instant::now();
        let signature = self.state_signature();
        if signature != self.signature {
            self.render();
        }
    }
    fn state_signature(&self) -> (bool, bool, String) {
        (
            unsafe { accessibility_sys::AXIsProcessTrusted() },
            TEST_BUSY.load(Ordering::SeqCst),
            {
                let cfg = crate::config::load();
                format!(
                    "{}:{}:{}",
                    cfg.translation_engine.key(),
                    cfg.translation_target.key(),
                    localization::is_english()
                )
            },
        )
    }
    unsafe fn demo_scene(&self, root: id, interactive: bool) {
        let line = label(
            root,
            "A little curiosity changes everything.",
            rect(445., 350., 490., 47.),
            26.,
            false,
            false,
        );
        let font: id = msg_send![class!(NSFont),fontWithName:string("Georgia") size:26f64];
        if font != nil {
            let _: () = msg_send![line,setFont:font];
        }
        let attributed: id = msg_send![line, attributedStringValue];
        let highlighted: id = msg_send![attributed, mutableCopy];
        let _: () = msg_send![highlighted,addAttribute:string("NSBackgroundColor") value:color(1.,0.86,0.43,0.65) range:cocoa::foundation::NSRange::new(9,9)];
        let _: () = msg_send![line,setAttributedStringValue:highlighted];
        let _: () = msg_send![highlighted, release];
        let bee = button(
            root,
            self.delegate,
            "",
            rect(624., 313., 44., 44.),
            Action::Demo,
            false,
            true,
            "beek-onboarding-demo",
        );
        let small_icon: id = msg_send![self.icon, copy];
        let _: () = msg_send![small_icon,setSize:NSSize::new(28.,28.)];
        let _: () = msg_send![bee,setImage:small_icon];
        let _: () = msg_send![small_icon, release];
        let _: () = msg_send![bee,setImagePosition:1u64];
        let _: () = msg_send![bee,setImageScaling:2u64];
        let bee_layer: id = msg_send![bee, layer];
        let white: id = msg_send![color(1., 1., 1., 0.96), CGColor];
        let _: () = msg_send![bee_layer,setBackgroundColor:white];
        let _: () = msg_send![bee_layer,setShadowOpacity:0.12f32];
        let _: () = msg_send![bee_layer,setShadowRadius:8f64];
        let _: () = msg_send![bee,setAccessibilityLabel:string("点击小蜜蜂，查看演示译文")];
        let _: () = msg_send![bee,setToolTip:string("本地演示，不发送网络请求")];
        if !interactive || self.demo_revealed {
            let panel = surface(root, rect(464., 196., 340., 114.), 22., 0.90, true);
            label(
                panel,
                "一点好奇心，改变一切。",
                rect(23., 59., 294., 35.),
                25.,
                false,
                false,
            );
            let divider = surface(panel, rect(24., 42., 292., 0.5), 0., 1., false);
            let layer: id = msg_send![divider, layer];
            let gray: id = msg_send![color(0.3, 0.4, 0.5, 0.12), CGColor];
            let _: () = msg_send![layer,setBackgroundColor:gray];
            picture(panel, self.icon, rect(24., 15., 20., 20.));
            label(panel, "BeeK", rect(53., 12., 100., 25.), 15., false, true);
        } else {
            label(
                root,
                "点一下小蜜蜂，试试看。",
                rect(515., 243., 330., 26.),
                16.,
                false,
                true,
            );
        }
    }
    fn render(&mut self) {
        let _pool = crate::macos_runtime::AutoreleasePool::new();
        self.signature = self.state_signature();
        let config = crate::config::load();
        let engine = config.translation_engine;
        let page_changed = self.rendered_step != Some(self.step);
        self.rendered_step = Some(self.step);
        unsafe {
            let _: () = msg_send![self.window,setTitle:string("BeeK 使用引导")];
            let root: id = msg_send![class!(NSView), alloc];
            let root: id = msg_send![root,initWithFrame:rect(0.,0.,self.width,self.height)];
            // One coordinate space, uniformly scaled to fit the active display.
            let _: () = msg_send![root,setBoundsSize:NSSize::new(960.,680.)];
            let _: () = msg_send![root,setWantsLayer:YES];
            picture(root, self.backdrop, rect(0., 0., 960., 680.));
            picture(root, self.icon, rect(52., 545., 50., 50.));
            label(root, "BeeK", rect(116., 542., 170., 51.), 31., true, false);
            language_picker(
                root,
                self.delegate,
                rect(747., 627., 165., 28.),
                &UiLanguage::ALL.map(|l| l.label()),
                UiLanguage::ALL
                    .iter()
                    .position(|l| *l == config.ui_language)
                    .unwrap_or(0),
                1,
                "beek-ui-language",
            );
            if self.step > 0 {
                button(
                    root,
                    self.delegate,
                    "上一步",
                    rect(824., 578., 88., 30.),
                    Action::Back,
                    false,
                    true,
                    "beek-onboarding-back",
                );
            }
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
            label(
                root,
                title,
                rect(50., 326., 385., 145.),
                if localization::is_english() { 44. } else { 48. },
                true,
                false,
            );
            label(root, body, rect(52., 249., 370., 70.), 18., false, true);
            match self.step {
                0 => self.demo_scene(root, false),
                1 => {
                    self.demo_scene(root, true);
                    label(
                        root,
                        if self.signature.0 {
                            "辅助功能已开启，可以自动取词。"
                        } else {
                            "开启辅助功能，让 BeeK 读到选中的文字。"
                        },
                        rect(52., 199., 370., 30.),
                        13.,
                        false,
                        true,
                    );
                    button(
                        root,
                        self.delegate,
                        if self.signature.0 {
                            "查看辅助功能设置"
                        } else {
                            "开启辅助功能…"
                        },
                        rect(43., 155., 208., 35.),
                        Action::Permissions,
                        false,
                        true,
                        "beek-onboarding-permission",
                    );
                    label(
                        root,
                        "演示只在本机完成，不调用翻译服务。",
                        rect(480., 137., 410., 25.),
                        12.,
                        false,
                        true,
                    );
                }
                2 => {
                    label(
                        root,
                        "调用使用你的火山方舟账号额度。\n也可以先体验无需 Key 的免费百度。",
                        rect(52., 192., 367., 47.),
                        13.,
                        false,
                        true,
                    );
                    let panel = surface(root, rect(461., 205., 440., 332.), 24., 0.80, false);
                    label(
                        panel,
                        "连接豆包，只需三步",
                        rect(26., 276., 390., 30.),
                        21.,
                        true,
                        false,
                    );
                    for (index, (heading, detail)) in [
                        ("创建 API Key", "在火山方舟控制台创建你的密钥。"),
                        ("选择豆包模型", "开通模型，复制 Model ID 或接入点 ID。"),
                        ("填入 BeeK，保存并启用", "模型可留空，使用默认豆包模型。"),
                    ]
                    .iter()
                    .enumerate()
                    {
                        let y = 215. - index as f64 * 76.;
                        label(
                            panel,
                            &format!("0{}", index + 1),
                            rect(26., y, 35., 27.),
                            13.,
                            true,
                            true,
                        );
                        label(panel, heading, rect(73., y, 338., 29.), 16., true, false);
                        label(
                            panel,
                            detail,
                            rect(73., y - 24., 338., 25.),
                            13.,
                            false,
                            true,
                        );
                    }
                    button(
                        root,
                        self.delegate,
                        "打开火山方舟",
                        rect(471., 158., 171., 34.),
                        Action::Console,
                        false,
                        true,
                        "beek-onboarding-console",
                    );
                    button(
                        root,
                        self.delegate,
                        "API Key 官方说明",
                        rect(675., 158., 195., 34.),
                        Action::Guide,
                        false,
                        true,
                        "beek-onboarding-guide",
                    );
                    label(
                        root,
                        &format!(
                            "{}{}",
                            localization::tr("当前服务："),
                            localization::tr(engine.label())
                        ),
                        rect(477., 106., 422., 26.),
                        12.,
                        false,
                        true,
                    );
                    button(
                        root,
                        self.delegate,
                        "切换其他翻译服务…",
                        rect(472., 66., 230., 34.),
                        Action::Other,
                        false,
                        true,
                        "beek-onboarding-other",
                    );
                }
                _ => {
                    label(
                        root,
                        if self.signature.0 {
                            "辅助功能已开启，可以在其他应用中取词。"
                        } else {
                            "开启辅助功能后，才能自动显示浮动按钮。"
                        },
                        rect(52., 204., 366., 34.),
                        13.,
                        false,
                        true,
                    );
                    button(
                        root,
                        self.delegate,
                        "查看辅助功能设置",
                        rect(43., 164., 211., 34.),
                        Action::Permissions,
                        false,
                        true,
                        "beek-onboarding-permission",
                    );
                    let panel = surface(root, rect(461., 174., 440., 400.), 24., 0.90, true);
                    label(
                        panel,
                        "体验你的第一句翻译",
                        rect(25., 340., 390., 33.),
                        23.,
                        true,
                        false,
                    );
                    label(
                        panel,
                        &format!(
                            "{}{}",
                            localization::tr("当前服务："),
                            localization::tr(engine.label())
                        ),
                        rect(26., 260., 389., 32.),
                        12.,
                        false,
                        true,
                    );
                    label(panel, "翻译为", rect(26., 301., 94., 24.), 14., false, true);
                    language_picker(
                        panel,
                        self.delegate,
                        rect(119., 297., 204., 30.),
                        &TargetLanguage::ALL.map(|l| l.label()),
                        TargetLanguage::ALL
                            .iter()
                            .position(|l| *l == config.translation_target)
                            .unwrap_or(0),
                        0,
                        "beek-translation-target",
                    );
                    let example = label(
                        panel,
                        sample(config.translation_target),
                        rect(26., 203., 389., 56.),
                        21.,
                        false,
                        false,
                    );
                    let _: () = msg_send![example,setSelectable:YES];
                    label(
                        panel,
                        "无需选中英文，点击下方按钮即可。",
                        rect(26., 170., 389., 24.),
                        13.,
                        true,
                        true,
                    );
                    let result = self.trial_message();
                    label(panel, &result, rect(26., 85., 389., 75.), 14., false, true);
                    button(
                        panel,
                        self.delegate,
                        if self.signature.1 {
                            "正在翻译…"
                        } else {
                            "点击试译"
                        },
                        rect(26., 24., 190., 44.),
                        Action::Trial,
                        true,
                        !self.signature.1,
                        "beek-onboarding-trial",
                    );
                    button(
                        root,
                        self.delegate,
                        "更换翻译服务…",
                        rect(469., 138., 213., 34.),
                        Action::Other,
                        false,
                        true,
                        "beek-onboarding-other",
                    );
                }
            }
            if !self.notice.is_empty() {
                label(
                    root,
                    &self.notice,
                    rect(52., 483., 368., 38.),
                    12.,
                    false,
                    true,
                );
            }
            let (next, a) = match self.step {
                0 => ("开始体验", Action::Next),
                1 => ("选择翻译服务", Action::Next),
                2 => ("连接豆包…", Action::Configure),
                _ => ("开始使用 BeeK", Action::Finish),
            };
            let identifier = if self.step == 2 {
                "beek-onboarding-configure"
            } else {
                "beek-onboarding-next"
            };
            button(
                root,
                self.delegate,
                next,
                rect(50., 86., 260., 54.),
                a,
                true,
                true,
                identifier,
            );
            for (index, a) in [
                Action::Welcome,
                Action::Usage,
                Action::Services,
                Action::Ready,
            ]
            .iter()
            .enumerate()
            {
                let view = button(
                    root,
                    self.delegate,
                    "",
                    rect(52. + index as f64 * 38., 44., 30., 6.),
                    *a,
                    false,
                    true,
                    &format!("beek-onboarding-step-{index}"),
                );
                let layer: id = msg_send![view, layer];
                let tint: id = msg_send![
                    if self.step == index {
                        color(0.15, 0.16, 0.17, 1.)
                    } else {
                        color(0.12, 0.15, 0.18, 0.13)
                    },
                    CGColor
                ];
                let _: () = msg_send![layer,setBackgroundColor:tint];
                let _: () = msg_send![view,setAccessibilityLabel:string(["欢迎","使用方式","添加豆包模型","权限与试译"][index])];
                let _: () = msg_send![view,setToolTip:string(["欢迎","使用方式","添加豆包模型","权限与试译"][index])];
            }
            button(
                root,
                self.delegate,
                if self.step == 2 {
                    "先用当前服务"
                } else {
                    "稍后设置"
                },
                rect(252., 32., if self.step == 2 { 160. } else { 124. }, 28.),
                if self.step == 2 {
                    Action::Next
                } else {
                    Action::Skip
                },
                false,
                true,
                if self.step == 2 {
                    "beek-onboarding-next"
                } else {
                    "beek-onboarding-skip"
                },
            );
            let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
            let reduced_motion: objc::runtime::BOOL =
                msg_send![workspace, accessibilityDisplayShouldReduceMotion];
            if page_changed && reduced_motion == NO {
                let _: () = msg_send![root,setAlphaValue:0f64];
            }
            let _: () = msg_send![self.window,setContentView:root];
            if page_changed && reduced_motion == NO {
                let _: () = msg_send![class!(NSAnimationContext), beginGrouping];
                let context: id = msg_send![class!(NSAnimationContext), currentContext];
                let _: () = msg_send![context,setDuration:0.22f64];
                let animator: id = msg_send![root, animator];
                let _: () = msg_send![animator,setAlphaValue:1f64];
                let _: () = msg_send![class!(NSAnimationContext), endGrouping];
            }
            let _: () = msg_send![root, release];
        }
    }
}
impl Drop for OnboardingWindow {
    fn drop(&mut self) {
        let _pool = crate::macos_runtime::AutoreleasePool::new();
        unsafe {
            let _: () = msg_send![self.window,setDelegate:nil];
            let _: () = msg_send![self.window, close];
            let _: () = msg_send![self.window, release];
            let _: () = msg_send![self.delegate, release];
            let _: () = msg_send![self.backdrop, release];
            let _: () = msg_send![self.icon, release];
        }
        crate::menubar::set_dock_visible(crate::config::load().show_in_dock);
        crate::ax::pulse_self_ui();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn first_run_is_once_per_revision_and_diagnostics_do_not_open_windows() {
        assert!(super::should_show(0, false));
        assert!(!super::should_show(1, false));
        assert!(!super::should_show(50, false));
        assert!(!super::should_show(0, true));
    }
}
