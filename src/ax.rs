// BeeK AX 取词模块 — 从 v0.11 移植
// AX-only，不模拟按键，不读剪贴板
use accessibility_sys::*;
use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use std::ffi::c_void;

unsafe fn ax_get_attr(e: AXUIElementRef, n: &str) -> Result<*const c_void, i32> {
    let a = CFString::new(n);
    let mut v: *const c_void = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(e, a.as_concrete_TypeRef(), &mut v as *mut *const c_void);
    if err != 0 || v.is_null() { return Err(err); }
    Ok(v)
}

unsafe fn ax_param_attr(el: AXUIElementRef, attr: &str, param: *const c_void) -> Result<*const c_void, i32> {
    let a = CFString::new(attr);
    let mut v: *const c_void = std::ptr::null();
    let err = AXUIElementCopyParameterizedAttributeValue(el, a.as_concrete_TypeRef(), param as _, &mut v as *mut *const c_void);
    if err != 0 || v.is_null() { return Err(err); }
    Ok(v)
}

unsafe fn cfstring_to_string(v: *const c_void) -> Option<String> {
    if v.is_null() { return None; }
    let s = CFString::wrap_under_get_rule(v as CFStringRef);
    let text = s.to_string();
    if text.trim().is_empty() { return None; }
    if text.chars().count() > 2000 { return None; }
    Some(text)
}

unsafe fn try_get_text(el: AXUIElementRef) -> Option<String> {
    let v = ax_get_attr(el, kAXSelectedTextAttribute).ok()?;
    cfstring_to_string(v)
}

/// 选区矩形 (x, y, w, h)。坐标是「显示器左上角为原点、y 轴向下」的屏幕坐标，
/// 与 GPUI 的窗口坐标系一致（实测见 main.rs 的 gpui_rect 注释），不要当 Cocoa 左下原点处理。
#[derive(Clone, Copy, Debug)]
pub struct SelBounds { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }

fn debug_on() -> bool {
    std::env::var("BEEK_DEBUG").is_ok()
}

/// 当前焦点元素（取词与取矩形共用同一条链路）
unsafe fn focused_element() -> Option<AXUIElementRef> {
    focused_app_and_element().map(|(_, el)| el)
}

/// 焦点 app + 焦点元素。搜索选区的兜底路线需要 app（拿焦点窗口）
unsafe fn focused_app_and_element() -> Option<(AXUIElementRef, AXUIElementRef)> {
    let sys = AXUIElementCreateSystemWide();
    AXUIElementSetMessagingTimeout(sys, 0.5);
    let app = ax_get_attr(sys, kAXFocusedApplicationAttribute).ok()? as AXUIElementRef;
    let el = ax_get_attr(app, kAXFocusedUIElementAttribute).ok()? as AXUIElementRef;
    AXUIElementSetMessagingTimeout(el, 0.5);
    Some((app, el))
}

/// AXBoundsForRange：给一个 range 取屏幕矩形
unsafe fn bounds_from_range(el: AXUIElementRef, range: *const c_void) -> Option<SelBounds> {
    let val = ax_param_attr(el, "AXBoundsForRange", range).ok()?;
    let mut rect = [0f64; 4];
    // CGRect 的 type code 是 kAXValueTypeCGRect(3)，不是 kAXValueTypeCGSize(2)
    let ok: bool = AXValueGetValue(val as _, kAXValueTypeCGRect, rect.as_mut_ptr() as *mut c_void);
    if ok && rect[2] > 0. && rect[3] > 0. {
        Some(SelBounds { x: rect[0], y: rect[1], w: rect[2], h: rect[3] })
    } else {
        None
    }
}

/// 单个元素上的三条取矩形路线：原生 AXSelectedTextRange / WebKit·Chromium
/// AXSelectedTextMarkerRange / 多选区 AXSelectedTextRanges
unsafe fn bounds_routes(el: AXUIElementRef) -> Option<SelBounds> {
    if let Ok(r) = ax_get_attr(el, "AXSelectedTextRange") {
        if let Some(b) = bounds_from_range(el, r) {
            return Some(b);
        }
    }
    if let Ok(r) = ax_get_attr(el, "AXSelectedTextMarkerRange") {
        if let Some(b) = bounds_from_range(el, r) {
            return Some(b);
        }
    }
    if let Ok(arr) = ax_get_attr(el, "AXSelectedTextRanges") {
        let a = core_foundation::array::CFArray::<*const c_void>::wrap_under_get_rule(arr as _);
        if !a.is_empty() {
            if let Some(first) = a.get(0) {
                if let Some(b) = bounds_from_range(el, *first) {
                    return Some(b);
                }
            }
        }
    }
    None
}

/// 兜底：用元素自身矩形（取左下角一小块），保证图标至少出现在文本区域附近
unsafe fn element_frame(el: AXUIElementRef) -> Option<SelBounds> {
    let pos = ax_get_attr(el, kAXPositionAttribute).ok()?;
    let size = ax_get_attr(el, kAXSizeAttribute).ok()?;
    let mut p = [0f64; 2];
    let mut s = [0f64; 2];
    let okp: bool = AXValueGetValue(pos as _, kAXValueTypeCGPoint, p.as_mut_ptr() as *mut c_void);
    let oks: bool = AXValueGetValue(size as _, kAXValueTypeCGSize, s.as_mut_ptr() as *mut c_void);
    if okp && oks && s[0] > 0. && s[1] > 0. {
        Some(SelBounds { x: p[0], y: p[1], w: s[0].min(40.), h: s[1].min(20.) })
    } else {
        None
    }
}

/// 一次取回「选中文本 + 它的矩形」，两者必须来自**同一个元素**。
///
/// 为什么必须同一个元素：原来是两条独立链路 —— 文本走 `ax_selected_text()`（含树搜索），
/// 矩形走 `ax_selection_bounds()`（焦点元素 + 沿父链上溯）。云之家这类应用 AX 树很薄，
/// 结果是「文本来自某个元素、矩形来自另一个父容器的 range」，算出来的图标位置
/// 离真正的选区很远（用户实测：云之家里图标飘到屏幕角落，备忘录里正常）。
///
/// 现在的判定顺序（每一步都要求「这个元素自己有选中文本」）：
///   1. 焦点元素本身
///   2. 沿父链最多 4 层
///   3. 焦点元素子树 BFS（有些应用选区在子节点上）
/// 只有元素自己持有所选文本时，它的 range 矩形才可信；取不到矩形就用它自己的元素矩形。
/// 三条都不成立返回 None —— 调用方会用鼠标位置兜底，宁可贴着鼠标也不要乱飘。
#[derive(Clone, Debug)]
pub struct Selection {
    pub text: String,
    pub bounds: SelBounds,
    /// 矩形来源：element（元素自己的 range）/ element-frame（退化成元素框）/ tree（子树搜索到）
    pub source: &'static str,
}

fn rect_ok(b: &SelBounds) -> bool {
    b.w > 0. && b.h > 0. && b.x > -64. && b.y > -64. && b.x < 20000. && b.y < 20000.
}

/// 元素自己持有的选中文本（AXSelectedText 优先；Chromium/WebKit 只有 marker range 时走它）
unsafe fn element_text(el: AXUIElementRef) -> Option<String> {
    if let Some(t) = try_get_text(el) {
        return Some(t);
    }
    let range = ax_get_attr(el, "AXSelectedTextMarkerRange").ok()?;
    let v = ax_param_attr(el, "AXStringForRange", range).ok()?;
    cfstring_to_string(v)
}

/// 元素自己的矩形：range 矩形优先，退化为元素自身的 frame
unsafe fn element_bounds(el: AXUIElementRef) -> Option<(SelBounds, &'static str)> {
    if let Some(b) = bounds_routes(el) {
        if rect_ok(&b) {
            return Some((b, "element"));
        }
    }
    element_frame(el).filter(rect_ok).map(|b| (b, "element-frame"))
}

/// 一次遍历里找到的候选：文本 + 可选矩形。
/// 有文本但没矩形的节点不会被丢掉 —— 终端、Safari、Chromium 这类应用常常就是这种情况，
/// 位置交给调用方的鼠标锚点兜底（早期版本把它当硬条件，结果这些应用一个都取不到词）。
struct Hit {
    text: String,
    bounds: Option<SelBounds>,
    source: &'static str,
}

unsafe fn node_hit(el: AXUIElementRef, base: &'static str, want_webarea: bool) -> Option<Hit> {
    if want_webarea && get_role(el).map(|r| r != "AXWebArea").unwrap_or(true) {
        return None;
    }
    let text = element_text(el)?;
    let (bounds, source) = match element_bounds(el) {
        Some((b, sub)) => (Some(b), if sub == "element-frame" { "element-frame" } else { base }),
        None => (None, "no-rect"),
    };
    Some(Hit { text, bounds, source })
}

/// 树里有没有 AXWebArea（用来区分「这个应用压根不用 WebArea」和「有 WebArea 但没选中文本」）
unsafe fn has_webarea(el: AXUIElementRef, depth: u32, budget: &mut u32) -> bool {
    if depth > 8 || *budget == 0 {
        return false;
    }
    *budget -= 1;
    if get_role(el).map(|r| r == "AXWebArea").unwrap_or(false) {
        return true;
    }
    if let Ok(children) = ax_get_attr(el, kAXChildrenAttribute) {
        let arr = core_foundation::array::CFArray::<*const c_void>::wrap_under_get_rule(children as _);
        for i in 0..arr.len().min(50) {
            let child = *arr.get(i).unwrap() as AXUIElementRef;
            if has_webarea(child, depth + 1, budget) {
                return true;
            }
        }
    }
    false
}

/// 子树搜索。`want_webarea` = 只认 AXWebArea（Safari / Chromium 的选区挂在那里）。
unsafe fn search_tree(
    el: AXUIElementRef,
    depth: u32,
    budget: &mut u32,
    want_webarea: bool,
) -> Option<Hit> {
    if depth > 8 || *budget == 0 {
        return None;
    }
    *budget -= 1;
    if let Some(hit) = node_hit(el, "tree", want_webarea) {
        return Some(hit);
    }
    if let Ok(children) = ax_get_attr(el, kAXChildrenAttribute) {
        let arr = core_foundation::array::CFArray::<*const c_void>::wrap_under_get_rule(children as _);
        for i in 0..arr.len().min(50) {
            let child = *arr.get(i).unwrap() as AXUIElementRef;
            if let Some(hit) = search_tree(child, depth + 1, budget, want_webarea) {
                return Some(hit);
            }
        }
    }
    None
}

/// 给「取词失败」的日志用：当前前台应用叫什么、它的 AX 形态（有没有焦点元素 / 窗口 / WebArea）
pub unsafe fn describe_frontmost() -> String {
    let sys = AXUIElementCreateSystemWide();
    AXUIElementSetMessagingTimeout(sys, 0.5);
    let app = match ax_get_attr(sys, kAXFocusedApplicationAttribute) {
        Ok(a) => a as AXUIElementRef,
        Err(_) => return "拿不到系统级焦点应用".to_string(),
    };
    let name = el_string(app, kAXTitleAttribute);
    let has_el = ax_get_attr(app, kAXFocusedUIElementAttribute).is_ok();
    let win = ax_get_attr(app, kAXFocusedWindowAttribute)
        .ok()
        .map(|v| v as AXUIElementRef);
    let web = match win {
        Some(w) => {
            let mut budget: u32 = 800;
            has_webarea(w, 0, &mut budget)
        }
        None => false,
    };
    format!(
        "app={:?} 焦点元素={} 焦点窗口={} 有WebArea={}",
        name,
        if has_el { "有" } else { "无" },
        if win.is_some() { "有" } else { "无" },
        if web { "是" } else { "否" }
    )
}

/// 上一次成功拿到的前台应用 pid。
/// 用处：系统级「当前焦点应用」在焦点切换的一瞬间会返回 kAXErrorNoValue(-25212)，
/// 此时不能直接放弃本次取词（那样用户要反复划好几次才成功一次），
/// 沿用它 —— 用户刚在哪个应用里划词，那个应用就是我们要找的。
static LAST_APP_PID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// 已经打开过完整 AX 树的应用（pid → 已设置），避免每次都重复设置。
/// 参考豆包实现：它的 libaha_cua.dylib 里同时有 "AXEnhancedUserInterface" 和
/// "AXManualAccessibility" 两个字符串 —— 就是靠这两个应用级开关
/// 让 WebKit / Electron 把完整 AX 树（含选区）暴露出来。
/// 不设这两个开关时，Safari 这种应用整棵树里连一个「有选区的节点」都没有（实测）。
/// pid → 上次尝试打开 AX 树的时间。成功过的不再尝试；失败过的隔一段时间才再试
/// （不能让失败的 pid 每个 tick 都重试一次 —— 那样 20 次/秒地打 AX 接口，还会刷屏日志）
static AX_TREE_TRIED: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<i32, (std::time::Instant, bool)>>,
> = std::sync::OnceLock::new();

fn tried_map(
) -> &'static std::sync::Mutex<std::collections::HashMap<i32, (std::time::Instant, bool)>> {
    AX_TREE_TRIED.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// 该不该现在再尝试一次：从没试过 → 试；试过且成功 → 不再试；失败过 → 隔 60 秒再试
fn should_try(pid: i32) -> bool {
    let map = tried_map().lock().unwrap();
    match map.get(&pid) {
        None => true,
        Some((_, true)) => false,
        Some((at, false)) => at.elapsed() > std::time::Duration::from_secs(60),
    }
}

/// 给某个应用打开完整 AX 树。返回 true 表示这次是新开的（调用方可能要等一下再查）。
pub unsafe fn enable_ax_tree(pid: i32) -> bool {
    if pid <= 0 || !should_try(pid) {
        return false;
    }
    use core_foundation::boolean::CFBoolean;
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    let app = AXUIElementCreateApplication(pid);
    AXUIElementSetMessagingTimeout(app, 0.5);
    // Chromium / Electron 是「懒开启」：第一次设置 AXManualAccessibility 会返回
    // 不支持(-25205)或未实现，**应用收到这次调用后才去建 AX 树**，再设一次就成了（实测 Chrome/Muse）。
    // 所以这里每个属性都试两轮，中间等一下。
    // 另外实测 AXEnhancedUserInterface 在本机对所有应用都是 -25208（未实现），
    // 真正起作用的是 AXManualAccessibility —— 两个都试，哪个成都算。
    let mut ok = false;
    for round in 0..2 {
        for attr in ["AXManualAccessibility", "AXEnhancedUserInterface"] {
            let key = CFString::new(attr);
            let val = CFBoolean::true_value().as_CFType();
            let err =
                AXUIElementSetAttributeValue(app, key.as_concrete_TypeRef(), val.as_CFTypeRef());
            if err == 0 {
                ok = true;
                eprintln!("[beek] 打开 AX 树：{} 第 {} 轮成功", attr, round + 1);
            }
        }
        if ok {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
    }
    if !ok {
        eprintln!("[beek] 打开 AX 树失败：该应用两个属性都不支持（可能是非 WebKit/Chromium 应用）");
    }
    tried_map().lock().unwrap().insert(pid, (std::time::Instant::now(), ok));
    if ok {
        eprintln!("[beek] 已打开 pid {} 的完整 AX 树（AXManualAccessibility）", pid);
    }
    ok
}

/// pid_t 就是 i32；只为 AXUIElementGetPid 的签名服务，避免为它引 libc
type LibcPid = i32;

unsafe fn focused_app() -> Option<AXUIElementRef> {
    for _ in 0..3 {
        let sys = AXUIElementCreateSystemWide();
        AXUIElementSetMessagingTimeout(sys, 0.5);
        if let Ok(a) = ax_get_attr(sys, kAXFocusedApplicationAttribute) {
            let app = a as AXUIElementRef;
            let mut pid: LibcPid = 0;
            if AXUIElementGetPid(app, &mut pid) == 0 && pid > 0 {
                if pid == std::process::id() as LibcPid {
                    // 前台是我们自己（对话框/面板在前面）：绝对不要问自己，会死锁
                    log_throttled(
                        "焦点是自己",
                        3,
                        "[beek] 取词：前台是 BeeK 自己（弹框/面板在前面），本轮跳过 AX",
                    );
                    return None;
                }
                LAST_APP_PID.store(pid, std::sync::atomic::Ordering::Relaxed);
            }
            return Some(app);
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    // 重试仍失败（正在切焦点）→ 用上次成功的那个应用
    let pid = LAST_APP_PID.load(std::sync::atomic::Ordering::Relaxed);
    if pid > 0 {
        log_throttled(
            "沿用上次 pid",
            5,
            &format!(
                "[beek] 取词：系统级焦点应用暂时取不到（焦点切换中），沿用上次的 pid {}",
                pid
            ),
        );
        return Some(AXUIElementCreateApplication(pid));
    }
    // 注意：这个函数每 50ms 被调一次，消息必须节流，否则会在日志里刷屏
    // （实测：前台没有「别人的应用」时——比如我们自己的面板在最前面——会一直走到这里）
    log_throttled(
        "无焦点应用",
        5,
        "[beek] 取词：拿不到系统级焦点应用，也没有可沿用的应用",
    );
    None
}

/// 同一条消息最多每 `secs` 秒打一次
fn log_throttled(key: &'static str, secs: u64, msg: &str) {
    use std::sync::{Mutex, OnceLock};
    static LAST: OnceLock<Mutex<std::collections::HashMap<&'static str, std::time::Instant>>> =
        OnceLock::new();
    let map = LAST.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let mut guard = map.lock().unwrap();
    let now = std::time::Instant::now();
    if let Some(at) = guard.get(key) {
        if at.elapsed() < std::time::Duration::from_secs(secs) {
            return;
        }
    }
    guard.insert(key, now);
    eprintln!("{}", msg);
}

/// 「我们自己的界面正在最前面」这个判断，用**开关 + 时间窗口**两件东西表达。
///
/// 为什么需要它：AX 查询的目标如果**是我们自己**，AppKit 会在**调用线程里**遍历自己的
/// 视图来回答（栈上是 `-[NSApplication accessibilityHitTest:]`），那要 NSViewHierarchyLock；
/// 主线程此刻可能正在做 display cycle 的布局刷新、也要这把锁 → 两边互等，主线程卡死
/// （表现就是转圈、菜单/弹框点了没反应）。
///
/// 为什么还要时间窗口：真正踩到的路径是 `AXUIElementCopyElementAtPosition` ——
/// 用户点了我们自己对话框上的按钮，那**一下的坐标**就在我们窗口上；哪怕对话框已经关掉，
/// 轮询拿着这个坐标去问，问到的还是我们自己。所以要「关掉之后再留一段宽限」。
/// 实测：不留宽限时，点「已保存」对话框的「好」之后必卡。
static SELF_UI_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SELF_UI_GRACE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 关掉之后再宽限这么久（轮询每 50ms 一次，够覆盖「刚点完那一下」）
const SELF_UI_GRACE: u64 = 1500;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn grace_until() -> u64 {
    SELF_UI_GRACE_MS.load(std::sync::atomic::Ordering::Relaxed)
}

/// 我们自己的界面打开/关闭（对话框、菜单）。关闭时留一段宽限，见上面的说明。
pub fn set_self_ui_front(on: bool) {
    use std::sync::atomic::Ordering;
    SELF_UI_OPEN.store(on, Ordering::Relaxed);
    if !on {
        SELF_UI_GRACE_MS.store(now_ms() + SELF_UI_GRACE, Ordering::Relaxed);
    }
}

/// 我们自己的窗口上刚发生一次点击（图标窗口 / 弹窗）——只延长宽限窗口
pub fn pulse_self_ui() {
    SELF_UI_GRACE_MS.store(now_ms() + SELF_UI_GRACE, std::sync::atomic::Ordering::Relaxed);
}

/// 现在要不要跳过 AX 查询（我们自己的界面在前台，或刚点过我们自己的界面）
fn self_ui_in_front() -> bool {
    SELF_UI_OPEN.load(std::sync::atomic::Ordering::Relaxed) || now_ms() < grace_until()
}

/// 按屏幕坐标取元素：`AXUIElementCopyElementAtPosition`。
///
/// 这条是参考豆包实现学来的（它的 libaha_cua.dylib 导入了这个 API）：
/// 不用去问「系统级焦点元素」——Safari 这类应用根本不提供 app 级焦点元素
/// （实测返回 nil），只问「鼠标下面是谁」就绕开了这个坑。
/// 坐标是 AX 屏幕坐标：左上原点、y 向下，与 mouse::position_topleft() 一致。
unsafe fn element_at_position(x: f64, y: f64) -> Option<AXUIElementRef> {
    let sys = AXUIElementCreateSystemWide();
    AXUIElementSetMessagingTimeout(sys, 0.5);
    let mut el: AXUIElementRef = std::ptr::null_mut();
    let err = AXUIElementCopyElementAtPosition(sys, x as f32, y as f32, &mut el);
    if err == 0 && !el.is_null() {
        AXUIElementSetMessagingTimeout(el, 0.5);
        Some(el)
    } else {
        None
    }
}

/// Identify the owner of the press before allowing mouse-driven copy fallbacks.
pub unsafe fn point_owner_pid(point: (f64, f64)) -> Option<i32> {
    let el = element_at_position(point.0, point.1)?;
    let mut pid = 0;
    let result = (AXUIElementGetPid(el, &mut pid) == 0 && pid > 0).then_some(pid);
    core_foundation::base::CFRelease(el as _);
    result
}

/// 给「取词失败」日志用：鼠标位置那个元素是什么、它有没有选中文本。
/// 注意：这条信息只有在**目标应用处于活动状态**时才有意义 ——
/// 从终端里跑探针时目标应用往往不是活动应用，那时它不会报选区（会误导判断）。
pub unsafe fn describe_at_position(x: f64, y: f64) -> String {
    match element_at_position(x, y) {
        Some(el) => format!(
            "鼠标处 role={} 选区={}",
            get_role(el).unwrap_or_else(|| "?".to_string()),
            el_selection_desc(el).unwrap_or_else(|| "无".to_string())
        ),
        None => "鼠标处取不到元素".to_string(),
    }
}

/// 取当前选区（文本 + 矩形）。
///
/// 两条硬约束（都是实测踩出来的）：
/// 1. **不能在拿不到 app 级焦点元素时提前返回**：Safari 不提供 app 级 AXFocusedUIElement，
///    只能在「焦点窗口」里搜 AXWebArea。早期版本在第一步就 `?` 掉，Safari/Muse/Grok Bot 全废。
/// 2. **有文本但没矩形也要算命中**：终端、Safari、Chromium 常常给不出 range 矩形
///    （AXBoundsForRange 直接失败）。早期版本要求同一元素既能给文本又能给矩形，
///    结果这些应用一个都取不到词。
/// `deep` = 是否允许做「焦点窗口级的深层搜索」（遍历上百个 AX 节点，比较贵）。
/// 监视器每 tick 都调这个函数，所以平时靠 `deep=false` 只走便宜的路
/// （焦点元素 → 父链 → 焦点元素子树）；只有刚发生过划词动作时才放开深层搜索。
pub unsafe fn ax_selection(deep: bool, point: Option<(f64, f64)>) -> Option<Selection> {
    ax_selection_for_pid(deep, point, None)
}

/// AXUIElement identity is stable across repeated copies of the same focused window.
pub unsafe fn focused_window_key(pid: i32) -> u64 {
    use core_foundation::base::{CFHash, CFRelease};
    let app = AXUIElementCreateApplication(pid);
    AXUIElementSetMessagingTimeout(app, 0.5);
    let key = match ax_get_attr(app, kAXFocusedWindowAttribute) {
        Ok(window) => {
            let key = CFHash(window) as u64;
            CFRelease(window);
            key
        }
        Err(_) => 0,
    };
    CFRelease(app as _);
    key
}

/// The live monitor supplies a foreground PID. Diagnostics may use the legacy focus lookup.
pub unsafe fn ax_selection_for_pid(deep: bool, point: Option<(f64, f64)>, expected_pid: Option<i32>) -> Option<Selection> {
    // 自己的界面在最前面（对话框/菜单开着）→ 直接不问 AX：
    // 问自己的视图会和主线程的布局刷新抢锁，死锁（见 SELF_FRONTMOST）
    if self_ui_in_front() {
        log_throttled(
            "自己在前台",
            3,
            "[beek] 取词：我们自己的界面在前台 / 刚被点过 → 本轮跳过 AX（问自己会和主线程抢锁）",
        );
        return None;
    }
    // ① 先问「鼠标下面是谁」——豆包的路线。这一条不依赖焦点元素，
    //    对 Safari 这类不提供焦点元素的应用是唯一可行的 AX 路子。
    if let Some((x, y)) = point {
        if let Some(el) = element_at_position(x, y) {
            let mut point_pid = 0;
            // Dock/menu elements belong to a different process. Do not treat them as text,
            // and do not reuse an unrelated cached click to query another application's tree.
            let same_owner = expected_pid.map(|pid| AXUIElementGetPid(el, &mut point_pid) == 0 && point_pid == pid).unwrap_or(true);
            if same_owner {
                let mut cur = el;
                for level in 0..5 {
                    let base: &'static str = if level == 0 { "at-position" } else { "at-position-parent" };
                    if let Some(hit) = node_hit(cur, base, false) {
                        match hit.bounds {
                            // 有文本也有矩形：直接用
                            Some(bounds) => {
                                return Some(Selection { text: hit.text, bounds, source: hit.source })
                            }
                            // 只有文本：也认（位置交给鼠标锚点兜底）
                            None => {
                                let win = ax_get_attr(cur, kAXWindowAttribute)
                                    .ok()
                                    .map(|v| v as AXUIElementRef);
                                let bounds = win
                                    .and_then(|w| element_frame(w))
                                    .unwrap_or(SelBounds { x, y, w: 0., h: 0. });
                                return Some(Selection {
                                    text: hit.text,
                                    bounds,
                                    source: "no-rect",
                                });
                            }
                        }
                    }
                    match ax_get_attr(cur, kAXParentAttribute) {
                        Ok(p) => cur = p as AXUIElementRef,
                        Err(_) => break,
                    }
                }
            }
        }
    }

    // Runtime requests use a fixed owner; never fall back to another application's old selection.
    let app = match expected_pid {
        Some(pid) => AXUIElementCreateApplication(pid),
        None => focused_app()?,
    };
    let pid_now = expected_pid.unwrap_or_else(|| LAST_APP_PID.load(std::sync::atomic::Ordering::Relaxed));
    let just_enabled = pid_now > 0 && enable_ax_tree(pid_now);
    let focused_el = ax_get_attr(app, kAXFocusedUIElementAttribute)
        .ok()
        .map(|v| v as AXUIElementRef);
    let win = ax_get_attr(app, kAXFocusedWindowAttribute)
        .ok()
        .map(|v| v as AXUIElementRef);

    // 候选顺序：焦点元素自己 → 父链 → 焦点元素子树 → 焦点窗口里的 AXWebArea → 焦点窗口子树
    let mut candidates: Vec<Hit> = Vec::new();
    let mut text_only: Option<Hit> = None;
    // 用普通函数而不是闭包：闭包会同时可变借走 candidates，后面就没法再读它
    fn record(hit: Option<Hit>, with_rect: &mut Vec<Hit>, text_only: &mut Option<Hit>) {
        if let Some(h) = hit {
            if h.bounds.is_some() {
                with_rect.push(h);
            } else if text_only.is_none() {
                *text_only = Some(h);
            }
        }
    }

    if let Some(el) = focused_el {
        AXUIElementSetMessagingTimeout(el, 0.5);
        let mut cur = el;
        for level in 0..5 {
            let base: &'static str = if level == 0 { "element" } else { "parent" };
            record(node_hit(cur, base, false), &mut candidates, &mut text_only);
            if !candidates.is_empty() {
                break;
            }
            match ax_get_attr(cur, kAXParentAttribute) {
                Ok(p) => cur = p as AXUIElementRef,
                Err(_) => break,
            }
        }
        if candidates.is_empty() {
            let mut budget: u32 = 400;
            record(search_tree(el, 0, &mut budget, false), &mut candidates, &mut text_only);
        }
    }
    if candidates.is_empty() && deep {
        // 先焦点窗口，再（找不到时）该应用的其它窗口：
        // Safari / Muse 这类应用可能开了多个窗口或标签页，选区不一定在焦点窗口里
        let mut windows: Vec<AXUIElementRef> = Vec::new();
        if let Some(w) = win {
            windows.push(w);
        }
        if let Some(ws) = expected_pid.is_none().then(|| ax_get_attr(app, kAXWindowsAttribute).ok()).flatten() {
            let arr = core_foundation::array::CFArray::<*const c_void>::wrap_under_get_rule(ws as _);
            for i in 0..arr.len().min(6) {
                let w = *arr.get(i).unwrap() as AXUIElementRef;
                if Some(w) != win {
                    windows.push(w);
                }
            }
        }
        for (idx, w) in windows.iter().enumerate() {
            let mut budget: u32 = 800;
            record(search_tree(*w, 0, &mut budget, true), &mut candidates, &mut text_only);
            if candidates.is_empty() {
                let mut budget: u32 = 800;
                record(search_tree(*w, 0, &mut budget, false), &mut candidates, &mut text_only);
            }
            if !candidates.is_empty() {
                eprintln!("[beek] 取词：选区在第 {} 个窗口里找到（共扫 {} 个）", idx + 1, windows.len());
                break;
            }
        }
        if candidates.is_empty() && deep {
            // 刚给这个应用打开 AX 树时，它重建树需要一点时间 → 等 200ms 再查一次
            if just_enabled {
                std::thread::sleep(std::time::Duration::from_millis(200));
                for w in windows.iter() {
                    let mut budget: u32 = 800;
                    record(search_tree(*w, 0, &mut budget, true), &mut candidates, &mut text_only);
                    if candidates.is_empty() {
                        let mut budget: u32 = 800;
                        record(search_tree(*w, 0, &mut budget, false), &mut candidates, &mut text_only);
                    }
                    if !candidates.is_empty() {
                        eprintln!("[beek] 取词：打开 AX 树后重查，拿到了选区");
                        break;
                    }
                }
            }
            if candidates.is_empty() {
                eprintln!("[beek] 取词：扫了 {} 个窗口都没有 AX 选区", windows.len());
            }
        }
    }

    let chosen = candidates.into_iter().next().or(text_only);
    match chosen {
        Some(hit) => {
            let bounds = hit.bounds.unwrap_or_else(|| {
                // 连元素框都拿不到：用焦点窗口的矩形当占位（位置最终由鼠标锚点决定）
                win.and_then(|w| element_frame(w))
                    .unwrap_or(SelBounds { x: 0., y: 0., w: 0., h: 0. })
            });
            let source = if hit.bounds.is_none() { "no-rect" } else { hit.source };
            Some(Selection { text: hit.text, bounds, source })
        }
        None => {
            // 最后：app 自己的 AXSelectedText（同样没有矩形）
            if let Ok(v) = ax_get_attr(app, kAXSelectedTextAttribute) {
                if let Some(text) = cfstring_to_string(v) {
                    return Some(Selection {
                        text,
                        bounds: win
                            .and_then(|w| element_frame(w))
                            .unwrap_or(SelBounds { x: 0., y: 0., w: 0., h: 0. }),
                        source: "no-rect",
                    });
                }
            }
            if deep {
                if let Some(win) = win {
                    let mut budget: u32 = 800;
                    if has_webarea(win, 0, &mut budget) {
                    eprintln!(
                        "[beek] 取词：焦点窗口里有 AXWebArea，但它没有任何选中文本 \
                         → 该应用这次没有把选区暴露给 AX"
                        );
                    }
                }
            }
            None
        }
    }
}

/// 兼容旧入口：只要文本
pub unsafe fn ax_selected_text() -> Option<String> {
    ax_selection(true, None).map(|s| s.text)
}

/// 兼容旧入口：只要矩形
pub unsafe fn ax_selection_bounds() -> Option<SelBounds> {
    ax_selection(true, None).map(|s| s.bounds)
}

unsafe fn get_role(el: AXUIElementRef) -> Option<String> {
    let v = ax_get_attr(el, kAXRoleAttribute).ok()?;
    cfstring_to_string(v)
}

/// WebArea 兜底：Chromium/WebKit 里选区常挂在 AXWebArea 上
unsafe fn find_webarea_text(el: AXUIElementRef, depth: u32) -> Option<String> {
    if depth > 8 { return None; }
    if let Some(role) = get_role(el) {
        if role == "AXWebArea" {
            if let Some(t) = try_get_text(el) { return Some(t); }
            if let Ok(range) = ax_get_attr(el, "AXSelectedTextMarkerRange") {
                if let Ok(v) = ax_param_attr(el, "AXStringForRange", range) {
                    if let Some(t) = cfstring_to_string(v) { return Some(t); }
                }
            }
        }
    }
    if let Ok(children) = ax_get_attr(el, kAXChildrenAttribute) {
        let arr = core_foundation::array::CFArray::<*const c_void>::wrap_under_get_rule(children as _);
        for i in 0..arr.len().min(50) {
            let child = *arr.get(i).unwrap() as AXUIElementRef;
            if let Some(t) = find_webarea_text(child, depth + 1) { return Some(t); }
        }
    }
    None
}

/// 请求辅助功能权限：未授权时会弹出系统对话框引导用户去授权
pub unsafe fn prompt_accessibility() {
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    extern "C" {
        static kAXTrustedCheckOptionPrompt: CFStringRef;
    }
    let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
    let dict =
        CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
    let _ = AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef());
}

/// URL 不弹窗
pub fn is_url(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("http://") || t.starts_with("https://") || t.starts_with("www.")
}

// ---------- 诊断探针（BEEK_AX_PROBE=1）----------
//
// 用途：某个应用「划词弹不出图标」时，用它把该应用真实的 AX 情况打出来：
// 焦点元素链上有没有选中文本、有没有 AXWebArea、各节点的 role/frame、
// AXBoundsForRange 能不能出矩形。不做任何 UI 操作，不翻译，打印完就退出。

unsafe fn el_string(el: AXUIElementRef, attr: &str) -> String {
    match ax_get_attr(el, attr) {
        Ok(v) => cfstring_to_string(v).unwrap_or_else(|| "(空)".to_string()),
        Err(e) => format!("(err {})", e),
    }
}

unsafe fn el_frame(el: AXUIElementRef) -> String {
    let pos = ax_get_attr(el, kAXPositionAttribute).ok();
    let size = ax_get_attr(el, kAXSizeAttribute).ok();
    let (mut p, mut z) = ([0f64; 2], [0f64; 2]);
    let okp = pos.map(|v| AXValueGetValue(v as _, kAXValueTypeCGPoint, p.as_mut_ptr() as *mut c_void)).unwrap_or(false);
    let okz = size.map(|v| AXValueGetValue(v as _, kAXValueTypeCGSize, z.as_mut_ptr() as *mut c_void)).unwrap_or(false);
    if okp && okz {
        format!("({:.0},{:.0} {:.0}x{:.0})", p[0], p[1], z[0], z[1])
    } else {
        "(无 frame)".to_string()
    }
}

/// 这个节点自己有没有选中文本；有的话顺便试一下矩形
unsafe fn el_selection_desc(el: AXUIElementRef) -> Option<String> {
    let direct = match ax_get_attr(el, kAXSelectedTextAttribute) {
        Ok(v) => cfstring_to_string(v),
        Err(_) => None,
    };
    let marker = match ax_get_attr(el, "AXSelectedTextMarkerRange") {
        Ok(r) => match ax_param_attr(el, "AXStringForRange", r) {
            Ok(v) => cfstring_to_string(v),
            Err(_) => None,
        },
        Err(_) => None,
    };
    let text = direct.clone().or(marker.clone())?;
    let bounds = bounds_routes(el)
        .map(|b| format!("矩形 ok ({:.0},{:.0} {:.0}x{:.0})", b.x, b.y, b.w, b.h))
        .unwrap_or_else(|| "矩形 失败".to_string());
    let head: String = text.chars().take(30).collect();
    Some(format!(
        "文本={:?}（AXSelectedText={} marker={}）{}",
        head,
        if direct.is_some() { "有" } else { "无" },
        if marker.is_some() { "有" } else { "无" },
        bounds
    ))
}

unsafe fn probe_walk(el: AXUIElementRef, depth: u32, visited: &mut u32, hits: &mut u32) {
    if depth > 10 || *visited > 300 {
        return;
    }
    *visited += 1;
    let role = get_role(el).unwrap_or_else(|| "?".to_string());
    let is_web = role == "AXWebArea";
    if let Some(desc) = el_selection_desc(el) {
        *hits += 1;
        eprintln!("[探针] 有选区的节点：role={} frame={} {}", role, el_frame(el), desc);
    } else if is_web {
        eprintln!("[探针] AXWebArea：frame={} 标题={:?}（本身没有选中文本）", el_frame(el), el_string(el, kAXTitleAttribute));
    }
    if let Ok(children) = ax_get_attr(el, kAXChildrenAttribute) {
        let arr = core_foundation::array::CFArray::<*const c_void>::wrap_under_get_rule(children as _);
        for i in 0..arr.len().min(50) {
            let child = *arr.get(i).unwrap() as AXUIElementRef;
            probe_walk(child, depth + 1, visited, hits);
        }
    }
}

pub unsafe fn ax_probe() {
    eprintln!("[探针] 辅助功能权限：{}", AXIsProcessTrusted());
    // 可以指定 pid（BEEK_AX_PROBE_PID）去探某个应用；不指定就用当前焦点应用。
    // 指定 pid 很有用：探针一般是在终端里跑的，那时焦点应用是终端而不是目标应用。
    let pid: Option<i32> = std::env::var("BEEK_AX_PROBE_PID")
        .ok()
        .and_then(|v| v.trim().parse().ok());
    let (app, focused_el) = match pid {
        Some(p) => {
            let newly = enable_ax_tree(p);
            if newly {
                eprintln!("[探针] 已打开该应用的完整 AX 树，等 250ms 让它重建…");
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            let app = AXUIElementCreateApplication(p);
            let el = ax_get_attr(app, kAXFocusedUIElementAttribute)
                .ok()
                .map(|v| v as AXUIElementRef);
            (app, el)
        }
        None => match focused_app_and_element() {
            Some((a, e)) => (a, Some(e)),
            None => {
                eprintln!("[探针] 拿不到焦点 app（可用 BEEK_AX_PROBE_PID=<pid> 指定应用）");
                return;
            }
        },
    };
    eprintln!(
        "[探针] app={:?} pid={:?} 焦点元素={:?}",
        el_string(app, kAXTitleAttribute),
        pid,
        focused_el.map(|e| format!("role={} frame={}", get_role(e).unwrap_or_default(), el_frame(e)))
    );

    if let Some(el) = focused_el {
        eprintln!("[探针] 焦点元素 selectedText={:?}", el_string(el, kAXSelectedTextAttribute));
        let mut cur = el;
        for level in 0..6 {
            eprintln!(
                "[探针] 焦点链 {}：role={} frame={} 选区={:?}",
                level,
                get_role(cur).unwrap_or_else(|| "?".to_string()),
                el_frame(cur),
                el_selection_desc(cur).unwrap_or_else(|| "无".to_string())
            );
            match ax_get_attr(cur, kAXParentAttribute) {
                Ok(p) => cur = p as AXUIElementRef,
                Err(_) => break,
            }
        }
    }

    // 窗口：优先焦点窗口；拿不到就把该 app 的所有窗口都扫一遍
    let mut roots: Vec<AXUIElementRef> = Vec::new();
    match ax_get_attr(app, kAXFocusedWindowAttribute) {
        Ok(w) => roots.push(w as AXUIElementRef),
        Err(_) => {
            if let Ok(ws) = ax_get_attr(app, kAXWindowsAttribute) {
                let arr = core_foundation::array::CFArray::<*const c_void>::wrap_under_get_rule(ws as _);
                for i in 0..arr.len().min(8) {
                    roots.push(*arr.get(i).unwrap() as AXUIElementRef);
                }
            }
        }
    }
    // 再报告一次「app 运行时真正走的那条路」：系统级焦点应用 → ax_selection()
    let sys = AXUIElementCreateSystemWide();
    if let Ok(a) = ax_get_attr(sys, kAXFocusedApplicationAttribute) {
        eprintln!(
            "[探针] 系统级焦点 app = {:?}（app 的监视器就是用这个决定的）",
            el_string(a as AXUIElementRef, kAXTitleAttribute)
        );
    } else {
        eprintln!("[探针] 系统级焦点 app 取不到");
    }
    // 鼠标位置的元素（豆包那条路）：用户保持选中时跑探针，就能看出这条路通不通
    {
        let (mx, my) = crate::mouse::position_topleft();
        if let Some(el) = element_at_position(mx, my) {
            eprintln!(
                "[探针] 鼠标位置 ({:.0},{:.0}) 的元素：role={} frame={} 选区={:?}",
                mx,
                my,
                get_role(el).unwrap_or_else(|| "?".to_string()),
                el_frame(el),
                el_selection_desc(el).unwrap_or_else(|| "无".to_string())
            );
            // 往上找 3 层，看有没有哪一层带选区
            let mut cur = el;
            for level in 0..3 {
                match ax_get_attr(cur, kAXParentAttribute) {
                    Ok(p) => {
                        cur = p as AXUIElementRef;
                        eprintln!(
                            "[探针]   父层 {}：role={} frame={} 选区={:?}",
                            level + 1,
                            get_role(cur).unwrap_or_else(|| "?".to_string()),
                            el_frame(cur),
                            el_selection_desc(cur).unwrap_or_else(|| "无".to_string())
                        );
                    }
                    Err(_) => break,
                }
            }
        } else {
            eprintln!("[探针] 鼠标位置取不到元素");
        }
    }
    match ax_selection(true, None) {
        Some(s) => eprintln!(
            "[探针] ax_selection() = Some(来源={} 文本={:?})",
            s.source,
            s.text.chars().take(20).collect::<String>()
        ),
        None => eprintln!("[探针] ax_selection() = None ← app 看不到选区的原因就在这里"),
    }
    if roots.is_empty() {
        eprintln!("[探针] 该 app 没有可扫的窗口");
        return;
    }
    for win in roots {
        eprintln!(
            "[探针] 扫窗口：frame={} title={:?}",
            el_frame(win),
            el_string(win, kAXTitleAttribute)
        );
        let (mut visited, mut hits) = (0u32, 0u32);
        probe_walk(win, 0, &mut visited, &mut hits);
        eprintln!("[探针] 该窗口：访问 {} 个节点，{} 个有选区", visited, hits);
    }
}

/// 监听模式（BEEK_AX_WATCH=1）：装 CGEventTap，在用户划词的**那一刻**报告 AX 现场。
///
/// 为什么需要它：从终端里跑一次探针得到的结论是无效的 —— 那时目标应用不是活动应用，
/// 它不会报选区。这个模式在用户真正操作（目标应用处于活动状态）的瞬间采集证据。
pub unsafe fn ax_watch() {
    eprintln!("[监听] 辅助功能权限：{}", AXIsProcessTrusted());
    if !crate::eventtap::spawn_listener() {
        eprintln!("[监听] 事件监听没装上，退出");
        return;
    }
    eprintln!("[监听] 请在目标应用里划一次词（双击 / 拖选 / 键盘选都行），我在那一刻报告现场…");
    let mut seen_seq = 0u64;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(30));
        let Some((seq, kind, x, y, _at)) = crate::eventtap::last_event() else {
            continue;
        };
        if seq == seen_seq {
            continue;
        }
        seen_seq = seq;
        // 只在「鼠标抬起 / 键盘抬起」那一刻看：那时选区刚形成
        let interesting =
            kind == crate::eventtap::EV_LEFT_MOUSE_UP || kind == crate::eventtap::EV_KEY_UP;
        if !interesting {
            continue;
        }
        std::thread::sleep(std::time::Duration::from_millis(120));
        let sys = AXUIElementCreateSystemWide();
        AXUIElementSetMessagingTimeout(sys, 0.5);
        let app = ax_get_attr(sys, kAXFocusedApplicationAttribute)
            .ok()
            .map(|a| a as AXUIElementRef);
        let who = app
            .map(|a| el_string(a, kAXTitleAttribute))
            .unwrap_or_else(|| "?".to_string());
        let pid = app.and_then(|a| {
            let mut p: i32 = 0;
            if AXUIElementGetPid(a, &mut p) == 0 && p > 0 {
                Some(p)
            } else {
                None
            }
        });
        eprintln!(
            "[监听] {} event：坐标 ({:.0},{:.0}) app={:?} pid={:?}",
            if kind == crate::eventtap::EV_KEY_UP { "键盘抬起" } else { "鼠标抬起" },
            x,
            y,
            who,
            pid
        );
        if let Some(p) = pid {
            enable_ax_tree(p);
        }
        if let Some(el) = element_at_position(x, y) {
            eprintln!(
                "[监听]   坐标处：role={} frame={} 选区={:?}",
                get_role(el).unwrap_or_else(|| "?".to_string()),
                el_frame(el),
                el_selection_desc(el).unwrap_or_else(|| "无".to_string())
            );
            let mut cur = el;
            for level in 0..4 {
                match ax_get_attr(cur, kAXParentAttribute) {
                    Ok(pp) => {
                        cur = pp as AXUIElementRef;
                        eprintln!(
                            "[监听]     父层 {}：role={} frame={} 选区={:?}",
                            level + 1,
                            get_role(cur).unwrap_or_else(|| "?".to_string()),
                            el_frame(cur),
                            el_selection_desc(cur).unwrap_or_else(|| "无".to_string())
                        );
                    }
                    Err(_) => break,
                }
            }
        } else {
            eprintln!("[监听]   坐标处取不到元素");
        }
        match ax_selection(true, Some((x, y))) {
            Some(s) => eprintln!(
                "[监听]   ax_selection() = Some(来源={} 文本={:?})",
                s.source,
                s.text.chars().take(20).collect::<String>()
            ),
            None => eprintln!("[监听]   ax_selection() = None"),
        }
    }
}


// ---------- 菜单拷贝（参考 SelectedTextKit / Copi 的做法）----------
//
// 为什么不只是合成 ⌘C：Electron / Safari 这类应用经常收不到合成按键，
// 但「按它自己的『拷贝』菜单项」是让**应用自己**执行拷贝 —— 走的是它的菜单体系，
// 不依赖它是否响应合成键。实测这也是开机就稳定的那条路。

unsafe fn identifier_of(el: AXUIElementRef) -> Option<String> {
    let v = ax_get_attr(el, "AXIdentifier").ok()?;
    cfstring_to_string(v)
}

unsafe fn children_of(el: AXUIElementRef) -> Vec<AXUIElementRef> {
    let mut out = Vec::new();
    if let Ok(children) = ax_get_attr(el, kAXChildrenAttribute) {
        let arr = core_foundation::array::CFArray::<*const c_void>::wrap_under_get_rule(children as _);
        for i in 0..arr.len().min(60) {
            out.push(*arr.get(i).unwrap() as AXUIElementRef);
        }
    }
    out
}

/// 「拷贝」菜单项的多语言标题（取自 SelectedTextKit 的本地化表，取常见语言）
const COPY_TITLES: &[&str] = &[
    "Copy", "拷贝", "复制", "拷貝", "複製", "コピー", "복사", "Copier", "Copiar", "Copia",
    "Kopieren", "Копировать", "Kopiëren", "Kopiera", "Kopioi", "Kopyala", "Salin", "Sao chép",
    "Kopiuj", "Másolás", "Kopírovat", "Копирај", "העתק", "نسخ", "کپی",
];

/// 这个元素是不是「拷贝」菜单项？
/// 两条匹配规则：identifier 恰好是动作 selector `copy:`，**或者**
/// 标题在拷贝标题表里且快捷键字符是 C。
/// 为什么需要第二条：AppKit 应用（如 Ghostty/TextEdit）的菜单项 identifier 是
/// 自动生成的 `_NS:63` 这种，根本不叫 `copy:`（实测）。
unsafe fn is_copy_item(el: AXUIElementRef) -> bool {
    if identifier_of(el).map(|i| i == "copy:").unwrap_or(false) {
        return true;
    }
    let title = match ax_get_attr(el, kAXTitleAttribute) {
        Ok(v) => match cfstring_to_string(v) {
            Some(t) => t,
            None => return false,
        },
        Err(_) => return false,
    };
    if !COPY_TITLES.iter().any(|t| *t == title) {
        return false;
    }
    // 有快捷键字符时必须就是 C（排除同名的其它项）；
    // 但不少应用（实测 Ghostty）根本不暴露 cmdChar，所以「没有」也算通过。
    match ax_get_attr(el, "AXMenuItemCmdChar")
        .ok()
        .and_then(|v| cfstring_to_string(v))
    {
        Some(c) => c.eq_ignore_ascii_case("C"),
        None => true,
    }
}

/// 深度优先找「拷贝」菜单项
unsafe fn find_copy_item(el: AXUIElementRef, depth: u32, budget: &mut u32) -> Option<AXUIElementRef> {
    if depth > 6 || *budget == 0 {
        return None;
    }
    *budget -= 1;
    if is_copy_item(el) {
        return Some(el);
    }
    for child in children_of(el) {
        if let Some(hit) = find_copy_item(child, depth + 1, budget) {
            return Some(hit);
        }
    }
    None
}

/// 在目标应用的菜单栏里找「拷贝」菜单项，返回 (菜单项, 是否可用)。
/// 菜单顺序按 SelectedTextKit 的策略：先第 4 个（通常是「编辑」），再左右交替展开，
/// 最后整棵菜单兜底 —— 这样既快又不依赖菜单标题的本地化文本。
pub unsafe fn menu_copy_item(pid: i32) -> Option<(AXUIElementRef, bool)> {
    let app = AXUIElementCreateApplication(pid);
    AXUIElementSetMessagingTimeout(app, 0.5);
    let menubar = ax_get_attr(app, kAXMenuBarAttribute).ok()? as AXUIElementRef;
    let items = children_of(menubar);
    if items.is_empty() {
        return None;
    }
    let mut order: Vec<usize> = Vec::new();
    let start = 3usize;
    if items.len() > start {
        order.push(start);
        let mut offset = 1usize;
        loop {
            let mut pushed = false;
            if start >= offset {
                order.push(start - offset);
                pushed = true;
            }
            if start + offset < items.len() {
                order.push(start + offset);
                pushed = true;
            }
            if !pushed {
                break;
            }
            offset += 1;
        }
        for i in 0..items.len() {
            if !order.contains(&i) {
                order.push(i);
            }
        }
    } else {
        order.extend(0..items.len());
    }
    // 找不到时把菜单结构打出来（identifier / 标题 / cmd 字符），便于对齐匹配规则
    let debug_menu = std::env::var("BEEK_MENU_DUMP").is_ok();
    for idx in order {
        let mut budget: u32 = 400;
        if let Some(item) = find_copy_item(items[idx], 0, &mut budget) {
            // 可用性：菜单项灰掉 = 当前没有可拷贝的内容（也就是没选中）
            let enabled = ax_get_attr(item, kAXEnabledAttribute)
                .ok()
                .map(|v| {
                    core_foundation::boolean::CFBoolean::wrap_under_get_rule(v as _)
                        == core_foundation::boolean::CFBoolean::true_value()
                })
                .unwrap_or(true);
            return Some((item, enabled));
        }
    }
    if debug_menu {
        for (i, bar_item) in items.iter().enumerate() {
            eprintln!(
                "[菜单] 菜单栏项 {}：title={:?} identifier={:?}",
                i,
                ax_get_attr(*bar_item, kAXTitleAttribute)
                    .ok()
                    .and_then(|v| cfstring_to_string(v)),
                identifier_of(*bar_item)
            );
        }
        for (i, bar_item) in items.iter().enumerate() {
            let subs = children_of(*bar_item);
            for sub in subs {
                let entries = children_of(sub);
                if entries.is_empty() {
                    continue;
                }
                eprintln!("[菜单] 菜单栏项 {} 下的菜单（{} 项）：", i, entries.len());
                for (k, e) in entries.iter().enumerate().take(24) {
                    eprintln!(
                        "[菜单]   {}: title={:?} cmdChar={:?} identifier={:?} role={}",
                        k,
                        ax_get_attr(*e, kAXTitleAttribute)
                            .ok()
                            .and_then(|v| cfstring_to_string(v)),
                        ax_get_attr(*e, "AXMenuItemCmdChar")
                            .ok()
                            .and_then(|v| cfstring_to_string(v)),
                        identifier_of(*e),
                        get_role(*e).unwrap_or_default()
                    );
                }
            }
        }
    }
    None
}

/// 对元素执行 AXPress（等于用户点了它）
pub unsafe fn press(el: AXUIElementRef) -> bool {
    let action = CFString::new("AXPress");
    AXUIElementPerformAction(el, action.as_concrete_TypeRef()) == 0
}
