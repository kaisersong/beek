//! Main-thread, modeless update notices. They never enter an AppKit modal loop.
use crate::update::{self, Release};
use beek::core::localization;
use cocoa::base::{id, nil};
use cocoa::foundation::{NSPoint, NSRect, NSString};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel, NO, YES};
use objc::{class, msg_send, sel, sel_impl};
use std::{cell::Cell, marker::PhantomData, path::PathBuf, rc::Rc, sync::OnceLock};

pub enum Choice {
    Dismiss,
    Open(String),
    Prepare(Release),
    Install(PathBuf, String),
}
pub struct Plan {
    pub title: String,
    pub body: String,
    pub buttons: Vec<(&'static str, Choice)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Idle,
    Checking,
    Preparing,
}
impl Operation {
    pub fn begin(&mut self, next: Self) -> bool {
        if *self != Self::Idle || next == Self::Idle {
            return false;
        }
        *self = next;
        true
    }
    pub fn progress(self) -> Plan {
        let (title, body) = if self == Self::Preparing {
            (
                "正在下载更新…",
                "下载和解压在后台进行，你可以继续使用 BeeK。",
            )
        } else {
            ("正在检查更新…", "正在连接 GitHub，检查可用更新。")
        };
        Plan {
            title: title.into(),
            body: body.into(),
            buttons: vec![("隐藏", Choice::Dismiss)],
        }
    }
}
pub fn checked(result: Result<Option<Release>, String>) -> Plan {
    match result {
        Err(error) => Plan {
            title: "检查更新失败".into(),
            body: localization::message("{}\n\n当前版本 v{}。", &[&error, update::CURRENT_VERSION]),
            buttons: vec![
                (
                    "打开 Release 页面",
                    Choice::Open(update::RELEASES_URL.into()),
                ),
                ("好", Choice::Dismiss),
            ],
        },
        Ok(None) => Plan {
            title: "暂时没有可用的新版本".into(),
            body: localization::message(
                "仓库 {} 还没有发布过 release。\n\n当前版本 v{}。发布页：{}",
                &[update::REPO, update::CURRENT_VERSION, update::RELEASES_URL],
            ),
            buttons: vec![
                (
                    "打开 Release 页面",
                    Choice::Open(update::RELEASES_URL.into()),
                ),
                ("好", Choice::Dismiss),
            ],
        },
        Ok(Some(rel)) if !update::is_newer(&rel.version, update::CURRENT_VERSION) => Plan {
            title: "已是最新版本".into(),
            body: localization::message(
                "当前 v{}，远端最新 {}。",
                &[update::CURRENT_VERSION, &rel.tag],
            ),
            buttons: vec![("好", Choice::Dismiss)],
        },
        Ok(Some(rel)) => {
            let notes = rel.notes.lines().take(12).collect::<Vec<_>>().join("\n");
            let body = localization::message(
                "当前 v{} → 新版本 {}\n\n{}{}",
                &[
                    update::CURRENT_VERSION,
                    &rel.tag,
                    &if notes.trim().is_empty() {
                        localization::tr("（这条 release 没写说明）\n").into()
                    } else {
                        format!("{}\n", notes.trim())
                    },
                    if rel.asset_url.is_some() {
                        ""
                    } else {
                        localization::tr("\n注意：这条 release 没有附带升级包，只能手动下载。")
                    },
                ],
            );
            let mut buttons = vec![];
            if rel.asset_url.is_some() {
                buttons.push(("下载并更新", Choice::Prepare(rel.clone())));
            }
            buttons.push(("打开 Release 页面", Choice::Open(rel.page_url.clone())));
            buttons.push(("稍后", Choice::Dismiss));
            Plan {
                title: localization::message("发现新版本 {}", &[&rel.tag]),
                body,
                buttons,
            }
        }
    }
}
pub fn failed(error: &str) -> Plan {
    Plan {
        title: "更新失败".into(),
        body: localization::message("{}\n\n可以从 Release 页面手动下载安装。", &[error]),
        buttons: vec![
            (
                "打开 Release 页面",
                Choice::Open(update::RELEASES_URL.into()),
            ),
            ("好", Choice::Dismiss),
        ],
    }
}
pub fn prepared(app: PathBuf, version: String) -> Plan {
    Plan {
        title: localization::message("已下载 v{}，现在更新？", &[&version]),
        body:
            "点「立即更新」后 BeeK 会退出，替换 /Applications/BeeK.app 并在几秒后自动启动新版本。"
                .into(),
        buttons: vec![
            ("立即更新", Choice::Install(app, version)),
            ("稍后", Choice::Dismiss),
        ],
    }
}

struct State {
    response: Cell<Option<usize>>,
}
unsafe fn state(this: &Object) -> Option<&State> {
    let ptr: *mut std::ffi::c_void = *this.get_ivar("noticeState");
    (ptr as *mut State).as_ref()
}
extern "C" fn respond(this: &Object, _: Sel, sender: id) {
    unsafe {
        if let Some(state) = state(this) {
            let tag: i64 = msg_send![sender, tag];
            if state.response.get().is_none() {
                state
                    .response
                    .set(Some(usize::try_from(tag).unwrap_or(usize::MAX)));
            }
        }
    }
}
extern "C" fn closed(this: &Object, _: Sel, _: id) {
    unsafe {
        if let Some(state) = state(this) {
            if state.response.get().is_none() {
                state.response.set(Some(usize::MAX));
            }
        }
    }
}
fn delegate_class() -> &'static Class {
    static CLASS: OnceLock<&'static Class> = OnceLock::new();
    CLASS.get_or_init(|| {
        let mut class = ClassDecl::new("BeeKUpdateNoticeDelegate", class!(NSObject)).unwrap();
        class.add_ivar::<*mut std::ffi::c_void>("noticeState");
        unsafe {
            class.add_method(
                sel!(updateNoticeResponse:),
                respond as extern "C" fn(&Object, Sel, id),
            );
            class.add_method(
                sel!(windowWillClose:),
                closed as extern "C" fn(&Object, Sel, id),
            );
        }
        class.register()
    })
}
unsafe fn string(value: &str) -> id {
    let value = NSString::alloc(nil).init_str(localization::tr(value));
    let _: id = msg_send![value, autorelease];
    value
}

pub struct Prompt {
    alert: id,
    window: id,
    delegate: id,
    state: Box<State>,
    plan: Plan,
    _main_thread: PhantomData<Rc<()>>,
}
impl Prompt {
    pub fn new(plan: Plan) -> Self {
        let _pool = crate::macos_runtime::AutoreleasePool::new();
        unsafe {
            let alert: id = msg_send![class!(NSAlert), new];
            let _: () = msg_send![alert, setMessageText:string(&plan.title)];
            let _: () = msg_send![alert, setInformativeText:string(&plan.body)];
            let _: () = msg_send![alert, setAlertStyle:1i64];
            let mut state = Box::new(State {
                response: Cell::new(None),
            });
            let delegate: id = msg_send![delegate_class(), new];
            (*delegate).set_ivar(
                "noticeState",
                &mut *state as *mut State as *mut std::ffi::c_void,
            );
            for (index, (label, _)) in plan.buttons.iter().enumerate() {
                let button: id = msg_send![alert, addButtonWithTitle:string(label)];
                let _: () = msg_send![button, setTag:index as i64];
                let _: () = msg_send![button, setTarget:delegate];
                let _: () = msg_send![button, setAction:sel!(updateNoticeResponse:)];
            }
            let _: () = msg_send![alert, layout];
            let window: id = msg_send![alert, window];
            let _: id = msg_send![window, retain];
            let _: () = msg_send![window, setReleasedWhenClosed:NO];
            let _: () = msg_send![window, setRestorable:NO];
            let _: () = msg_send![window, setTitle:string(&plan.title)];
            let _: () = msg_send![window, setDelegate:delegate];
            let _: () = msg_send![window, setLevel:3i64];
            let _: () = msg_send![window, setCollectionBehavior:2usize];
            Self {
                alert,
                window,
                delegate,
                state,
                plan,
                _main_thread: PhantomData,
            }
        }
    }
    pub fn show(&self) {
        let _pool = crate::macos_runtime::AutoreleasePool::new();
        crate::menubar::close_status_menu();
        crate::ax::pulse_self_ui();
        unsafe {
            let screens: id = msg_send![class!(NSScreen), screens];
            let count: usize = msg_send![screens, count];
            let mouse: NSPoint = msg_send![class!(NSEvent), mouseLocation];
            let mut screen: id = msg_send![class!(NSScreen), mainScreen];
            for index in 0..count.min(32) {
                let candidate: id = msg_send![screens, objectAtIndex:index];
                let frame: NSRect = msg_send![candidate, frame];
                if mouse.x >= frame.origin.x
                    && mouse.x < frame.origin.x + frame.size.width
                    && mouse.y >= frame.origin.y
                    && mouse.y < frame.origin.y + frame.size.height
                {
                    screen = candidate;
                    break;
                }
            }
            if screen != nil {
                let available: NSRect = msg_send![screen, visibleFrame];
                let frame: NSRect = msg_send![self.window, frame];
                let point = NSPoint::new(
                    available.origin.x + (available.size.width - frame.size.width).max(0.) / 2.,
                    available.origin.y + (available.size.height - frame.size.height).max(0.) / 2.,
                );
                let _: () = msg_send![self.window, setFrameOrigin:point];
            }
            let app: id = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, setActivationPolicy:0i64];
            let _: () = msg_send![app, activateIgnoringOtherApps:YES];
            let _: () = msg_send![self.window, makeKeyAndOrderFront:nil];
            let _: () = msg_send![self.window, orderFrontRegardless];
        }
    }
    pub fn take_choice(&mut self) -> Option<Choice> {
        let index = self.state.response.take()?;
        Some(
            self.plan
                .buttons
                .get_mut(index)
                .map(|(_, choice)| std::mem::replace(choice, Choice::Dismiss))
                .unwrap_or(Choice::Dismiss),
        )
    }

    pub fn native_state(&self) -> serde_json::Value {
        let _pool = crate::macos_runtime::AutoreleasePool::new();
        unsafe {
            let app: id = msg_send![class!(NSApplication), sharedApplication];
            let modal: id = msg_send![app, modalWindow];
            let visible: i8 = msg_send![self.window, isVisible];
            let key: i8 = msg_send![self.window, isKeyWindow];
            let frame: NSRect = msg_send![self.window, frame];
            let screens: id = msg_send![class!(NSScreen), screens];
            let count: usize = msg_send![screens, count];
            let on_screen = (0..count).any(|index| {
                let screen: id = msg_send![screens, objectAtIndex:index];
                let available: NSRect = msg_send![screen, visibleFrame];
                frame.origin.x >= available.origin.x - 1.
                    && frame.origin.y >= available.origin.y - 1.
                    && frame.origin.x + frame.size.width
                        <= available.origin.x + available.size.width + 1.
                    && frame.origin.y + frame.size.height
                        <= available.origin.y + available.size.height + 1.
            });
            let buttons: id = msg_send![self.alert, buttons];
            let n: usize = msg_send![buttons, count];
            let wired = (0..n).all(|index| {
                let button: id = msg_send![buttons, objectAtIndex:index];
                let target: id = msg_send![button, target];
                let tag: i64 = msg_send![button, tag];
                target == self.delegate && tag == index as i64
            });
            let menu = crate::menubar::update_menu_state();
            serde_json::json!({"visible":visible != 0,"key":key != 0,"modal":modal != nil,"on_screen":on_screen,"buttons_wired":wired,"enabled_actions":menu.0,"disabled_actions":menu.1})
        }
    }

    fn diagnostic_dismiss(&self) {
        assert!(matches!(
            self.plan.buttons.last(),
            Some((_, Choice::Dismiss))
        ));
        unsafe {
            let buttons: id = msg_send![self.alert, buttons];
            let button: id = msg_send![buttons, objectAtIndex:self.plan.buttons.len() - 1];
            respond(&*self.delegate, sel!(updateNoticeResponse:), button);
        }
    }
}

/// Bounded in-process native test. It drives the production event path with
/// public fixtures, skips AX/model/auto-update workers and only selects Dismiss.
pub struct Probe {
    step: usize,
    since: std::time::Instant,
    retired: Option<usize>,
    records: Vec<serde_json::Value>,
    language_checks: Vec<serde_json::Value>,
}
impl Probe {
    pub fn new() -> Self {
        Self {
            step: 0,
            since: std::time::Instant::now(),
            retired: None,
            records: vec![],
            language_checks: vec![],
        }
    }
    pub fn tick(
        &mut self,
        prompt: &Option<Prompt>,
        operation: Operation,
        tx: &std::sync::mpsc::Sender<crate::UiEvent>,
    ) -> bool {
        if self.since.elapsed() < std::time::Duration::from_millis(250) {
            return false;
        }
        if let Some(retired) = self.retired {
            if prompt.is_some() {
                return false;
            }
            let _pool = crate::macos_runtime::AutoreleasePool::new();
            unsafe {
                let app: id = msg_send![class!(NSApplication), sharedApplication];
                let windows: id = msg_send![app, windows];
                let count: usize = msg_send![windows, count];
                assert!(
                    !(0..count).any(|i| {
                        let w: id = msg_send![windows,objectAtIndex:i];
                        w as usize == retired
                    }),
                    "closed update window retained"
                );
            }
            self.retired = None;
            self.step += 1;
            let event = match self.step {
                3 => crate::UiEvent::UpdateChecked(Ok(None)),
                4 => crate::UiEvent::UpdateChecked(Err("Public network fixture".into())),
                5 => crate::UiEvent::UpdateChecked(Ok(Some(Release {
                    version: "999.0.0".into(),
                    tag: "v999.0.0".into(),
                    notes: "Public notes".into(),
                    page_url: update::RELEASES_URL.into(),
                    asset_url: Some("https://example.com/public.zip".into()),
                }))),
                6 => crate::UiEvent::UpdateChecked(Ok(Some(Release {
                    version: "999.0.0".into(),
                    tag: "v999.0.0".into(),
                    notes: "Public notes".into(),
                    page_url: update::RELEASES_URL.into(),
                    asset_url: None,
                }))),
                7 => crate::UiEvent::UpdatePrepared(
                    PathBuf::from("public-fixture/BeeK.app"),
                    "999.0.0".into(),
                ),
                8 => crate::UiEvent::UpdateFailed("Public download fixture".into()),
                _ => {
                    let report = std::env::var("BEEK_UPDATE_UI_REPORT")
                        .expect("native UI report path required");
                    std::fs::write(report, serde_json::to_vec_pretty(&serde_json::json!({"passed":true,"states":self.records,"language_menus":self.language_checks,"closed_windows_released":true})).unwrap()).unwrap();
                    return true;
                }
            };
            tx.send(event).unwrap();
            self.since = std::time::Instant::now();
            return false;
        }
        if self.step == 0 {
            let languages = crate::menubar::language_menu_state();
            eprintln!("[beek] Native language menu validation: {languages}");
            for key in ["target", "ui"] {
                assert_eq!(languages[key]["enabled"], true, "{key} submenu disabled");
                assert_eq!(languages[key]["choices"], languages[key]["selectable"]);
                assert_eq!(languages[key]["selected"].as_array().unwrap().len(), 1);
            }
            self.language_checks = crate::menubar::diagnostic_language_menu_roundtrip();
            tx.send(crate::UiEvent::CheckUpdate).unwrap();
            tx.send(crate::UiEvent::CheckUpdate).unwrap();
            self.step = 1;
            self.since = std::time::Instant::now();
            return false;
        }
        let Some(prompt) = prompt.as_ref() else {
            return false;
        };
        if self.step == 1 && operation != Operation::Checking {
            panic!("progress window was not observable; delay the public feed fixture");
        }
        if self.step == 2 && operation != Operation::Idle {
            return false;
        }
        let record = prompt.native_state();
        assert_eq!(record["modal"], false, "update notice entered a modal loop");
        for key in ["visible", "key", "on_screen", "buttons_wired"] {
            assert_eq!(record[key], true, "{key}");
        }
        assert_eq!(record["disabled_actions"], 0);
        assert!(record["enabled_actions"].as_u64().unwrap() > 0);
        self.records.push(record);
        if self.step == 1 {
            self.step = 2;
            self.since = std::time::Instant::now();
            return false;
        }
        self.retired = Some(prompt.window as usize);
        prompt.diagnostic_dismiss();
        self.since = std::time::Instant::now();
        false
    }
}
impl Drop for Prompt {
    fn drop(&mut self) {
        let _pool = crate::macos_runtime::AutoreleasePool::new();
        unsafe {
            (*self.delegate).set_ivar("noticeState", std::ptr::null_mut::<std::ffi::c_void>());
            let _: () = msg_send![self.window, setDelegate:nil];
            let buttons: id = msg_send![self.alert, buttons];
            let count: usize = msg_send![buttons, count];
            for index in 0..count {
                let button: id = msg_send![buttons, objectAtIndex:index];
                let _: () = msg_send![button, setTarget:nil];
            }
            let _: () = msg_send![self.window, close];
            let _: () = msg_send![self.alert, release];
            let _: () = msg_send![self.window, release];
            let _: () = msg_send![self.delegate, release];
        }
        unsafe {
            let app: id = msg_send![class!(NSApplication), sharedApplication];
            let windows: id = msg_send![app, windows];
            let count: usize = msg_send![windows, count];
            let another_window = (0..count).any(|index| {
                let window: id = msg_send![windows, objectAtIndex:index];
                let visible: i8 = msg_send![window, isVisible];
                let style: usize = msg_send![window, styleMask];
                visible != 0 && style & 1 != 0
            });
            if !another_window {
                crate::menubar::set_dock_visible(crate::config::load().show_in_dock);
            }
        }
        crate::ax::pulse_self_ui();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release(version: &str, asset: bool) -> Release {
        Release {
            version: version.into(),
            tag: format!("v{version}"),
            notes: "Public release notes".into(),
            page_url: update::RELEASES_URL.into(),
            asset_url: asset.then(|| "https://example.com/public.zip".into()),
        }
    }
    #[test]
    fn an_older_release_cannot_offer_a_downgrade() {
        let plan = checked(Ok(Some(release("0.0.1", true))));
        assert_eq!(plan.title, "已是最新版本");
        assert!(matches!(plan.buttons.as_slice(), [(_, Choice::Dismiss)]));
    }
    #[test]
    fn no_release_and_errors_remain_dismissible_without_side_effects() {
        for plan in [
            checked(Ok(None)),
            checked(Err("Public network failure".into())),
            failed("Public download failure"),
        ] {
            assert!(matches!(plan.buttons.last(), Some((_, Choice::Dismiss))));
            assert!(!plan
                .buttons
                .iter()
                .any(|(_, action)| matches!(action, Choice::Prepare(_) | Choice::Install(..))));
        }
    }
    #[test]
    fn download_and_install_require_separate_explicit_choices() {
        let plan = checked(Ok(Some(release("999.0.0", true))));
        assert!(matches!(
            plan.buttons.first(),
            Some((_, Choice::Prepare(_)))
        ));
        assert!(!plan
            .buttons
            .iter()
            .any(|(_, action)| matches!(action, Choice::Install(..))));
        let plan = prepared(PathBuf::from("public-fixture/BeeK.app"), "999.0.0".into());
        assert!(matches!(
            plan.buttons.first(),
            Some((_, Choice::Install(..)))
        ));
        assert!(matches!(plan.buttons.last(), Some((_, Choice::Dismiss))));
    }
    #[test]
    fn missing_assets_only_offer_the_release_page() {
        assert!(!checked(Ok(Some(release("999.0.0", false))))
            .buttons
            .iter()
            .any(|(_, action)| matches!(action, Choice::Prepare(_))));
    }
    #[test]
    fn repeated_checks_and_downloads_cannot_create_parallel_manual_jobs() {
        let mut operation = Operation::Idle;
        assert!(operation.begin(Operation::Checking));
        for _ in 0..1000 {
            assert!(!operation.begin(Operation::Checking));
            assert!(!operation.begin(Operation::Preparing));
        }
        operation = Operation::Idle;
        assert!(operation.begin(Operation::Preparing));
        assert!(!operation.begin(Operation::Checking));
    }
}
