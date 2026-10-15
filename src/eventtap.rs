// 全局事件监听 —— 照豆包的做法（它的 libaha_cua.dylib 导入了 CGEventTapCreate）
//
// 为什么需要它：轮询（每 50ms 查一次 AX）永远踩不准用户的划词动作，
// 而 CGEventTap 能在**事件发生的那一刻**回调，并直接给出事件的屏幕坐标。
// 豆包用它来判断「什么时候该去看选区」；这里先用它做探针取证，
// 再决定要不要把运行时的触发也换过来。
//
// 权限：listen-only 的事件 tap 需要辅助功能权限（已有）；键盘事件还需要
// 输入监控权限，没有时 tap 会创建/启用失败，此时退回只看鼠标事件。
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

// kCGEventLeftMouseDown(1<<1) / LeftMouseUp(1<<2) / LeftMouseDragged(1<<6) / KeyUp(1<<11)
const MASK_MOUSE: u64 = (1 << 1) | (1 << 2) | (1 << 6);
const MASK_WITH_KEYS: u64 = MASK_MOUSE | (1 << 11);

const KCG_SESSION_EVENT_TAP: u32 = 1;
const KCG_HEAD_INSERT_EVENT_TAP: u32 = 0;
const KCG_EVENT_TAP_OPTION_LISTEN_ONLY: u32 = 1;

/// 事件类型（与 CGEventType 对应，只关心这几个）
pub const EV_LEFT_MOUSE_DOWN: u32 = 1;
pub const EV_LEFT_MOUSE_UP: u32 = 2;
pub const EV_LEFT_MOUSE_DRAGGED: u32 = 6;
pub const EV_KEY_UP: u32 = 11;

extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: extern "C" fn(*mut std::ffi::c_void, u32, *mut std::ffi::c_void, *mut std::ffi::c_void) -> *mut std::ffi::c_void,
        user_info: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
    fn CGEventGetLocation(event: *mut std::ffi::c_void) -> CGPoint;
    fn CFMachPortCreateRunLoopSource(
        allocator: *const std::ffi::c_void,
        port: *mut std::ffi::c_void,
        order: i64,
    ) -> *mut std::ffi::c_void;
    fn CFRunLoopGetCurrent() -> *mut std::ffi::c_void;
    fn CFRunLoopAddSource(rl: *mut std::ffi::c_void, source: *mut std::ffi::c_void, mode: *const std::ffi::c_void);
    fn CFRunLoopRun();
    /// 真正的 run loop mode 常量（不能自己造一个同名字符串 —— 那样 source 不会被调度，
    /// run loop 里没有 source 就立刻返回，事件一个都收不到。实测踩过）
    static kCFRunLoopDefaultMode: *const std::ffi::c_void;
}

/// 最近一次事件：类型、屏幕坐标（左上原点，与 AX 一致）、时间
pub struct TapEvent {
    pub kind: u32,
    pub x: f64,
    pub y: f64,
    pub at: Instant,
}

static LAST: OnceLock<Mutex<Option<TapEvent>>> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);
static KEYS_OK: AtomicBool = AtomicBool::new(false);

fn last_slot() -> &'static Mutex<Option<TapEvent>> {
    LAST.get_or_init(|| Mutex::new(None))
}

pub fn last_event() -> Option<(u64, u32, f64, f64, Instant)> {
    let seq = SEQ.load(Ordering::Relaxed);
    if seq == 0 {
        return None;
    }
    let guard = last_slot().lock().unwrap();
    guard.as_ref().map(|e| (seq, e.kind, e.x, e.y, e.at))
}

pub fn keys_available() -> bool {
    KEYS_OK.load(Ordering::Relaxed)
}

extern "C" fn callback(
    _proxy: *mut std::ffi::c_void,
    kind: u32,
    event: *mut std::ffi::c_void,
    _user: *mut std::ffi::c_void,
) -> *mut std::ffi::c_void {
    // 事件被禁用时系统会发 type=0xFFFFFFFE，需要重新启用；这里只记录感兴趣的事件
    if kind == EV_LEFT_MOUSE_DOWN || kind == EV_LEFT_MOUSE_UP || kind == EV_LEFT_MOUSE_DRAGGED || kind == EV_KEY_UP {
        let p = unsafe { CGEventGetLocation(event) };
        let mut guard = last_slot().lock().unwrap();
        *guard = Some(TapEvent {
            kind,
            x: p.x,
            y: p.y,
            at: Instant::now(),
        });
        SEQ.fetch_add(1, Ordering::Relaxed);
    }
    event
}

/// 起一个线程装事件 tap 并跑 run loop。返回是否装上。
pub fn spawn_listener() -> bool {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return true;
    }
    std::thread::spawn(|| unsafe {
        // 先试带键盘事件（键盘选词也能抓到），失败则退回只有鼠标
        let mut tap = CGEventTapCreate(
            KCG_SESSION_EVENT_TAP,
            KCG_HEAD_INSERT_EVENT_TAP,
            KCG_EVENT_TAP_OPTION_LISTEN_ONLY,
            MASK_WITH_KEYS,
            callback,
            std::ptr::null_mut(),
        );
        if !tap.is_null() {
            KEYS_OK.store(true, Ordering::Relaxed);
        } else {
            tap = CGEventTapCreate(
                KCG_SESSION_EVENT_TAP,
                KCG_HEAD_INSERT_EVENT_TAP,
                KCG_EVENT_TAP_OPTION_LISTEN_ONLY,
                MASK_MOUSE,
                callback,
                std::ptr::null_mut(),
            );
            if tap.is_null() {
                eprintln!("[tap] 事件监听创建失败（需要辅助功能权限）");
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
            eprintln!("[tap] 键盘事件不可用（缺输入监控权限），只监听鼠标事件");
        }
        CGEventTapEnable(tap, true);
        eprintln!(
            "[tap] 事件监听已就绪（键盘={}）",
            if KEYS_OK.load(Ordering::Relaxed) { "有" } else { "无" }
        );
        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        let rl = CFRunLoopGetCurrent();
        CFRunLoopAddSource(rl, source, kCFRunLoopDefaultMode);
        eprintln!("[tap] run loop 已挂上事件源，等事件…");
        CFRunLoopRun();
    });
    // 给线程一点时间把 tap 装上
    std::thread::sleep(std::time::Duration::from_millis(150));
    last_slot().lock().is_ok()
}
