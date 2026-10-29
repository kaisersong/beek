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
    /// 直接把事件投给某个进程：Electron 类应用（Muse、Grok Bot）常常收不到
    /// HID 层投递的按键，但对「投给自己进程」的事件会正常处理
    fn CGEventPostToPid(pid: i32, event: *mut std::ffi::c_void);
    fn CFRelease(cf: *const std::ffi::c_void);
}

/// 'C' 键的虚拟键码；kCGEventFlagMaskCommand
const KEY_C: u16 = 8;
const FLAG_COMMAND: u64 = 1 << 20;
/// kCGHIDEventTap / kCGSessionEventTap：两种投递层级，
/// 有些应用只认其中一种（实测 ⌘C 时灵时不灵就是这个原因之一）
const HID_TAP: u32 = 0;
const SESSION_TAP: u32 = 1;
/// 按键按下到抬起的间隔：太短（原来是 15ms）时部分应用会忽略这次按键
const KEY_DWELL_MS: u64 = 40;

/// ⌘C 的三种投递方式，按「最像真人」到「最定向」依次尝试
#[derive(Clone, Copy)]
enum CopyRoute {
    /// HID 层：等价于用户真的按了键
    Hid,
    /// 直接投给前台进程：Electron 类应用常常只认这条
    Pid(i32),
    /// Session 层：个别应用只认这条
    Session,
}

fn pasteboard() -> id {
    unsafe { msg_send![class!(NSPasteboard), generalPasteboard] }
}

/// 读剪贴板里的字符串（没有字符串时返回 None）
pub fn read_string() -> Option<String> {
    let _pool = crate::macos_runtime::AutoreleasePool::new();
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
    let _pool = crate::macos_runtime::AutoreleasePool::new();
    unsafe {
        let pb = pasteboard();
        let _: () = msg_send![pb, clearContents];
        let _: () = msg_send![pb, setString: nsstr(s) forType: nsstr("public.utf8-plain-text")];
    }
}

fn nsstr(s: &str) -> id {
    unsafe {
        let value = NSString::alloc(nil).init_str(s);
        msg_send![value, autorelease]
    }
}

/// 当前前台应用的 pid（给 CGEventPostToPid 用）
pub fn frontmost_pid() -> Option<i32> {
    let _pool = crate::macos_runtime::AutoreleasePool::new();
    unsafe {
        let ws: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let app: id = msg_send![ws, frontmostApplication];
        if app == nil {
            return None;
        }
        let pid: i32 = msg_send![app, processIdentifier];
        if pid <= 0 {
            None
        } else {
            Some(pid)
        }
    }
}

fn post_copy(route: CopyRoute) {
    unsafe {
        for down in [true, false] {
            let ev = CGEventCreateKeyboardEvent(std::ptr::null(), KEY_C, down);
            if ev.is_null() {
                continue;
            }
            CGEventSetFlags(ev, FLAG_COMMAND);
            match route {
                CopyRoute::Hid => CGEventPost(HID_TAP, ev),
                CopyRoute::Session => CGEventPost(SESSION_TAP, ev),
                CopyRoute::Pid(pid) => CGEventPostToPid(pid, ev),
            }
            CFRelease(ev as *const std::ffi::c_void);
            std::thread::sleep(std::time::Duration::from_millis(KEY_DWELL_MS));
        }
    }
}

/// 给当前前台应用发一次 ⌘C（HID 层，等于用户真的按了）
pub fn send_copy() {
    post_copy(CopyRoute::Hid);
}

/// 等剪贴板变化（复制是异步写盘的，Safari 这种可能要几百毫秒）
fn wait_change(before: i64, budget_ms: u64) -> bool {
    let mut waited = 0;
    while waited < budget_ms {
        std::thread::sleep(std::time::Duration::from_millis(40));
        waited += 40;
        if change_count() != before {
            return true;
        }
    }
    false
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
    let _pool = crate::macos_runtime::AutoreleasePool::new();
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
    let _pool = crate::macos_runtime::AutoreleasePool::new();
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
            let _: () = msg_send![item, release];
        }
        let _: () = msg_send![pb, writeObjects: arr];
    }
}

/// 我们自己写剪贴板（弹窗里的「复制」按钮）之后记一下这次的 changeCount，
/// 剪贴板监听要忽略它，否则会把自己的复制当成「用户复制」再弹一次按钮
static OWN_WRITE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(-1);

pub fn note_own_write() {
    OWN_WRITE.store(change_count(), std::sync::atomic::Ordering::Relaxed);
}

fn is_own_write(count: i64) -> bool {
    OWN_WRITE.load(std::sync::atomic::Ordering::Relaxed) == count
}

/// 剪贴板监听（方案 A：用户自己按 ⌘C，我们就弹按钮）。
/// 返回 true 表示这次变化是我们自己的复制造成的，应当忽略。
pub fn is_self_inflicted(count: i64) -> bool {
    is_own_write(count)
}

/// 清空剪贴板（复制前它是空的，还原时就应该还是空的）
pub fn clear() {
    let _pool = crate::macos_runtime::AutoreleasePool::new();
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
pub fn change_count() -> i64 {
    let _pool = crate::macos_runtime::AutoreleasePool::new();
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
    let pid = frontmost_pid();
    // 依次尝试三种投递方式。每种的等待都放宽：复制是异步写剪贴板的，
    // 短了会把「其实成功了」误判成「没反应」（实测 Safari 就这样）。
    let routes: [CopyRoute; 3] = [
        CopyRoute::Hid,
        match pid {
            Some(p) => CopyRoute::Pid(p),
            None => CopyRoute::Session,
        },
        CopyRoute::Session,
    ];
    let mut changed = false;
    for (i, route) in routes.iter().enumerate() {
        post_copy(*route);
        if wait_change(before_count, 500) {
            changed = true;
            if i > 0 {
                eprintln!("[beek] 取词：第 {} 种 ⌘C 投递方式生效", i + 1);
            }
            break;
        }
        eprintln!(
            "[beek] 取词：第 {} 种 ⌘C 投递方式（{:?}）没反应",
            i + 1,
            match route {
                CopyRoute::Hid => "HID",
                CopyRoute::Session => "Session",
                CopyRoute::Pid(_) => "投递给进程",
            }
        );
    }
    if !changed {
        return None; // 三种都没反应 → 不动剪贴板
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
    // 端到端验一次「模拟 ⌘C 取选中文本」：先在某个应用里选好文字再跑这个自检。
    eprintln!(
        "[自检] 前台进程 pid={:?}，开始走两段式 ⌘C…",
        frontmost_pid()
    );
    match copy_selection() {
        Some(t) => eprintln!(
            "[自检] 两段式 ⌘C 取到：{:?}（{} 字符）→ 通过",
            t.chars().take(30).collect::<String>(),
            t.chars().count()
        ),
        None => eprintln!("[自检] 两段式 ⌘C 没取到内容（当时可能没有选中文字）"),
    }
    eprintln!("[自检] 结束，剪贴板内容：{:?}", read_string());
}
