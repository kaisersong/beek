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

/// 剪贴板整体快照（所有条目 × 所有类型，连图片/文件一起）。
///
/// 为什么不能只存字符串：用户可能刚截了图（剪贴板里是图片），只存字符串就存不到、
/// 也就还原不回去，而 ⌘C 照样会把图片覆盖掉 —— 结果就是「截图粘不出来了」。
///
/// 为什么存的是**数据副本**而不是 NSPasteboardItem 引用：
/// 直接留着 pasteboardItem 对象、clearContents 之后再 writeObjects 写回，
/// 会抛 ObjC 异常把进程干掉（实测 abort，剪贴板还被留在清空状态）。
/// 把每个类型的字节拷下来、还原时用 NSData 重建条目才稳。
pub struct Snapshot(Vec<Vec<(String, Vec<u8>)>>);

pub fn snapshot() -> Option<Snapshot> {
    unsafe {
        let pb = pasteboard();
        let items: id = msg_send![pb, pasteboardItems];
        if items == nil {
            return None;
        }
        let n: usize = msg_send![items, count];
        let mut out = Vec::new();
        for i in 0..n {
            let item: id = msg_send![items, objectAtIndex: i];
            let types: id = msg_send![item, types];
            let tn: usize = msg_send![types, count];
            let mut tv = Vec::new();
            for j in 0..tn {
                let t: id = msg_send![types, objectAtIndex: j];
                let data: id = msg_send![item, dataForType: t];
                if data == nil {
                    continue;
                }
                let len: usize = msg_send![data, length];
                let ptr: *const u8 = msg_send![data, bytes];
                if ptr.is_null() || len == 0 {
                    continue;
                }
                let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
                tv.push((cstring(t), bytes));
            }
            out.push(tv);
        }
        Some(Snapshot(out))
    }
}

/// 把快照整体写回剪贴板
pub fn restore(snap: &Snapshot) {
    unsafe {
        let pb = pasteboard();
        let _: () = msg_send![pb, clearContents];
        if snap.0.is_empty() {
            return;
        }
        let arr: id = msg_send![class!(NSMutableArray), array];
        for item_types in &snap.0 {
            if item_types.is_empty() {
                continue;
            }
            let item: id = msg_send![class!(NSPasteboardItem), new];
            for (t, bytes) in item_types {
                let data: id = msg_send![class!(NSData),
                    dataWithBytes: bytes.as_ptr() as *const std::os::raw::c_void
                    length: bytes.len()];
                let _: () = msg_send![item, setData: data forType: nsstr(t)];
            }
            let _: () = msg_send![arr, addObject: item];
        }
        let _: () = msg_send![pb, writeObjects: arr];
    }
}

/// 清空剪贴板（复制前它是空的，还原时就应该还是空的）
pub fn clear() {
    unsafe {
        let pb = pasteboard();
        let _: () = msg_send![pb, clearContents];
    }
}

/// NSString → String（快照里读 pasteboard type 用）
unsafe fn cstring(v: id) -> String {
    let ptr: *const std::os::raw::c_char = msg_send![v, UTF8String];
    if ptr.is_null() {
        String::new()
    } else {
        std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

/// 剪贴板变更计数：用它判断「内容到底变没变」。
/// 不能靠比较字符串 —— 如果 ⌘C 复制的是文件/图片（非文本内容），字符串读出来是空的，
/// 会被误判成「没变化」而跳过还原，剪贴板照样被覆盖（实测踩过：截图粘不出来）。
fn change_count() -> i64 {
    unsafe {
        let pb = pasteboard();
        msg_send![pb, changeCount]
    }
}

/// 走一遍「⌘C → 判断有没有变 → 读文本 → 整体还原」。
/// 剪贴板没变 → 返回 None 且**完全不碰剪贴板**（用户其实没选中东西）。
pub fn copy_selection() -> Option<String> {
    let before_items = snapshot();
    let before_count = change_count();
    send_copy();
    // 给应用一点时间写剪贴板
    std::thread::sleep(std::time::Duration::from_millis(120));
    if change_count() == before_count {
        return None; // 没变 → 不动剪贴板
    }
    // 变了 → 先把文本读出来，然后无论文本有没有，都把剪贴板整体还原
    let text = read_string();
    match before_items {
        Some(snap) => restore(&snap),
        None => clear(),
    }
    text.filter(|s| !s.trim().is_empty())
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
    // 关键机制的独立验证：整体快照 → 覆盖 → 还原，必须回到原内容。
    // 用户遇到的「截图粘不出来」就是这一步没做对（只存字符串、存不到图片）。
    let snap = snapshot();
    let original = read_string();
    write_string("BEEK-CLIP-OVERWRITE-TEST");
    let mid = read_string();
    match &snap {
        Some(s) => restore(s),
        None => clear(),
    }
    let back = read_string();
    eprintln!(
        "[自检] 快照/还原：覆盖后读到 {:?}，还原后读到 {:?}（期望 == 原内容 {:?}）→ {}",
        mid,
        back,
        original,
        if back == original { "通过" } else { "失败" }
    );
}
