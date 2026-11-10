//! Explicit browser consent, independent of selection polling and JavaScript settings.
use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use objc::{class, msg_send, sel, sel_impl};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

static REQUESTING: AtomicBool = AtomicBool::new(false);

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn AEDeterminePermissionToAutomateTarget(
        target: *const std::ffi::c_void,
        event_class: u32,
        event_id: u32,
        ask: u8,
    ) -> i32;
}

/// Called only from the main UI event loop, outside a GPUI App borrow.
pub fn choose_browser(tx: Sender<crate::UiEvent>) {
    let _pool = crate::macos_runtime::AutoreleasePool::new();
    if REQUESTING.load(Ordering::Acquire) {
        crate::dialogs::alert(
            "浏览器授权",
            "授权请求正在等待系统响应，请先处理系统授权提示。",
            &["好"],
        );
        return;
    }
    let browsers: Vec<_> = crate::appcopy::BROWSERS.iter().copied().filter(|(bundle, _)| unsafe {
        let value = NSString::alloc(nil).init_str(bundle);
        let apps: id = msg_send![class!(NSRunningApplication), runningApplicationsWithBundleIdentifier:value];
        let count: usize = msg_send![apps, count];
        let _: () = msg_send![value, release];
        count > 0
    }).collect();
    if browsers.is_empty() {
        crate::dialogs::alert("请先打开浏览器", "请先打开 Safari、Chrome 或其他支持的浏览器，再点击此菜单申请授权。自动化列表只显示已请求过权限的应用，不能手动添加 BeeK。", &["好"]);
        return;
    }
    let mut buttons: Vec<_> = browsers.iter().map(|(_, name)| *name).collect();
    buttons.push("取消");
    let selected = crate::dialogs::alert("选择要授权的浏览器", "选择浏览器后，请在系统提示中点击“允许”。已拒绝过的请求需要在系统设置中重新勾选。此操作不会读取网页或选区。", &buttons);
    let Some(&(bundle, name)) = browsers.get(selected) else {
        return;
    };
    if REQUESTING.swap(true, Ordering::AcqRel) {
        return;
    }
    // At most one worker, no queue or polling. The OS consent call can wait for
    // user interaction indefinitely; keep the slot until it actually returns.
    let result = std::thread::Builder::new()
        .name("browser-consent".into())
        .spawn(move || {
            let _pool = crate::macos_runtime::AutoreleasePool::new();
            let status = unsafe {
                let value = NSString::alloc(nil).init_str(bundle);
                // Convenience constructor is autoreleased; the pool owns its lifetime.
                let descriptor: id =
                    msg_send![class!(NSAppleEventDescriptor), descriptorWithBundleIdentifier:value];
                let _: () = msg_send![value, release];
                if descriptor == nil {
                    -50
                } else {
                    let address: *const std::ffi::c_void = msg_send![descriptor, aeDesc];
                    let wildcard = u32::from_be_bytes(*b"****");
                    if address.is_null() {
                        -50
                    } else {
                        AEDeterminePermissionToAutomateTarget(address, wildcard, wildcard, 1)
                    }
                }
            };
            let _ = tx.send(crate::UiEvent::AutomationPermissionDone(name, status));
            REQUESTING.store(false, Ordering::Release);
        });
    if result.is_err() {
        REQUESTING.store(false, Ordering::Release);
        crate::dialogs::alert("浏览器授权", "无法启动授权请求，请稍后重试。", &["好"]);
    }
}

pub fn show_result(name: &str, status: i32) {
    let _pool = crate::macos_runtime::AutoreleasePool::new();
    match status {
        0 => { crate::dialogs::alert("浏览器已授权", &format!("BeeK 已获得控制 {name} 的权限。\n\n如需通过 JavaScript 取词，还需在浏览器中开启“允许 Apple 事件中的 JavaScript”；也可以继续使用应用拷贝和剪贴板取词。"), &["好"]); }
        -1743 => {
            if crate::dialogs::alert("浏览器授权被拒绝", &format!("请在系统设置 → 隐私与安全性 → 自动化中展开 BeeK，开启 {name}。系统不会为已拒绝的请求重复弹出授权提示。"), &["打开系统设置", "取消"]) == 0 {
                crate::menubar::open_automation_panel();
            }
        }
        -600 => { crate::dialogs::alert("请先打开浏览器", "浏览器已退出，请重新打开后再申请授权。", &["好"]); }
        _ => { crate::dialogs::alert("浏览器授权未完成", &format!("系统返回错误 {status}。请确认 BeeK 已安装在“应用程序”中，然后重新打开浏览器并重试。"), &["好"]); }
    }
}
