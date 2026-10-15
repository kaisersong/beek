// BeeK 剪贴板兜底取词
//
// 为什么需要它：Safari / Muse / Grok Bot 这类应用**根本不把选区暴露给 AX**
// （探针实测：焦点窗口里有 AXWebArea，但它没有 AXSelectedText；
//  而 Chrome 的 AXWebArea 是有选中文本的）。
// 对这些应用，唯一可靠的取词方式就是「模拟 ⌘C，然后读剪贴板」。
//
// 三条自我约束，尽量不打扰用户：
// 1. **只在 AX 取不到时才走这条路**（AX 能拿到就完全不动剪贴板）
// 2. 复制前先把剪贴板原文存下来，读完立刻还原
// 3. 复制后剪贴板内容若与原文相同 → 说明用户其实没选中东西 → 什么也不做
use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use objc::{class, msg_send, sel, sel_impl};

// CoreGraphics 的 C 接口：只用到「造键盘事件 + 投递」两个函数，
// 直接声明就够，不必为它引一个 crate（cocoa 已经把 CoreGraphics 链进来了）
extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const std::ffi::c_void, key: u16, down: bool)
        -> *mut std::ffi::c_void;
    fn CGEventSetFlags(event: *mut std::ffi::c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
    fn CFRelease(cf: *const std::ffi::c_void);
}

/// 'C' 键的虚拟键码；kCGEventFlagMaskCommand；kCGHIDEventTap
const KEY_C: u16 = 8;
const FLAG_COMMAND: u64 = 1 << 20;
const HID_TAP: u32 = 0;

fn pasteboard() -> id {
    unsafe { msg_send![class!(NSPasteboard), generalPasteboard] }
}

/// 读剪贴板里的字符串（没有字符串时返回 None）
pub fn read_string() -> Option<String> {
    unsafe {
        let pb = pasteboard();
        let v: id = msg_send![pb, stringForType: nsstr("public.utf8-plain-text")];
        if v == nil {
            return None;
        }
        let ptr: *const std::os::raw::c_char = msg_send![v, UTF8String];
        if ptr.is_null() {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr(ptr)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

/// 写回字符串（还原剪贴板用）
pub fn write_string(s: &str) {
    unsafe {
        let pb = pasteboard();
        let _: () = msg_send![pb, clearContents];
        let _: () = msg_send![pb, setString: nsstr(s) forType: nsstr("public.utf8-plain-text")];
    }
}

fn nsstr(s: &str) -> id {
    unsafe { NSString::alloc(nil).init_str(s) }
}

/// 给当前前台应用发一次 ⌘C
pub fn send_copy() {
    unsafe {
        for down in [true, false] {
            let ev = CGEventCreateKeyboardEvent(std::ptr::null(), KEY_C, down);
            if ev.is_null() {
                continue;
            }
            CGEventSetFlags(ev, FLAG_COMMAND);
            CGEventPost(HID_TAP, ev);
            CFRelease(ev as *const std::ffi::c_void);
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
    }
}

/// 走一遍「⌘C → 读剪贴板 → 还原剪贴板」，返回这次复制到的文本。
/// 返回 None 表示剪贴板没有变化（用户其实没选中东西）或读不到。
pub fn copy_selection() -> Option<String> {
    let before = read_string();
    send_copy();
    // 给应用一点时间写剪贴板
    std::thread::sleep(std::time::Duration::from_millis(120));
    let after = read_string();
    if let Some(ref s) = before {
        if after.as_deref() == Some(s.as_str()) {
            return None;
        }
    }
    if let Some(ref b) = before {
        write_string(b); // 还原
    }
    after.filter(|s| !s.trim().is_empty())
}

/// 自检：读剪贴板 → 发 ⌘C → 再读 → 还原，把结果打出来。
/// 用来单独验证「模拟复制 + 剪贴板」这两个原语是否可用（不需要 AX）。
pub fn selftest() {
    let before = read_string();
    eprintln!(
        "[自检] 剪贴板原内容：{} 字符（{:?}）",
        before.as_ref().map(|s| s.chars().count()).unwrap_or(0),
        before
            .as_ref()
            .map(|s| s.chars().take(20).collect::<String>())
            .unwrap_or_default()
    );
    send_copy();
    std::thread::sleep(std::time::Duration::from_millis(200));
    let after = read_string();
    eprintln!(
        "[自检] 发过 ⌘C 之后读到的内容：{} 字符（{:?}）",
        after.as_ref().map(|s| s.chars().count()).unwrap_or(0),
        after
            .as_ref()
            .map(|s| s.chars().take(40).collect::<String>())
            .unwrap_or_default()
    );
    if let Some(b) = before {
        write_string(&b);
        eprintln!("[自检] 已还原剪贴板");
    }
}
