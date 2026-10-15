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
    let sys = AXUIElementCreateSystemWide();
    AXUIElementSetMessagingTimeout(sys, 0.5);
    let app = ax_get_attr(sys, kAXFocusedApplicationAttribute).ok()? as AXUIElementRef;
    let el = ax_get_attr(app, kAXFocusedUIElementAttribute).ok()? as AXUIElementRef;
    AXUIElementSetMessagingTimeout(el, 0.5);
    Some(el)
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

/// 单个元素自己的选区（文本 + 矩形）。文本为空或取不到矩形时返回 None。
unsafe fn element_selection(el: AXUIElementRef) -> Option<(String, SelBounds, &'static str)> {
    // 取文本：原生 AXSelectedText 优先；Chromium/WebKit 有时只给 marker range
    let text = match try_get_text(el) {
        Some(t) => t,
        None => {
            let range = ax_get_attr(el, "AXSelectedTextMarkerRange").ok()?;
            let v = ax_param_attr(el, "AXStringForRange", range).ok()?;
            cfstring_to_string(v)?
        }
    };
    if let Some(b) = bounds_routes(el) {
        if rect_ok(&b) {
            return Some((text, b, "element"));
        }
    }
    // 拿不到 range 矩形时退回元素矩形：位置一定落在文本区域内（可能偏一点，但不会飘走）
    let fb = element_frame(el)?;
    if rect_ok(&fb) {
        return Some((text, fb, "element-frame"));
    }
    None
}

/// 子树里找「自己有选中文本」的元素，并把它的文本 + 矩形一起带回来
unsafe fn tree_selection(el: AXUIElementRef, depth: u32) -> Option<(String, SelBounds, &'static str)> {
    if depth > 6 {
        return None;
    }
    if let Some(hit) = element_selection(el) {
        return Some((hit.0, hit.1, "tree"));
    }
    if let Ok(children) = ax_get_attr(el, kAXChildrenAttribute) {
        let arr = core_foundation::array::CFArray::<*const c_void>::wrap_under_get_rule(children as _);
        for i in 0..arr.len().min(50) {
            let child = *arr.get(i).unwrap() as AXUIElementRef;
            if let Some(hit) = tree_selection(child, depth + 1) {
                return Some((hit.0, hit.1, "tree"));
            }
        }
    }
    None
}

/// 取当前选区（文本 + 矩形，同一个元素）
pub unsafe fn ax_selection() -> Option<Selection> {
    let el = focused_element()?;
    let mut cur = el;
    for level in 0..5 {
        if let Some((text, bounds, base)) = element_selection(cur) {
            let source = if level == 0 { base } else { "parent" };
            return Some(Selection { text, bounds, source });
        }
        match ax_get_attr(cur, kAXParentAttribute) {
            Ok(p) => cur = p as AXUIElementRef,
            Err(_) => break,
        }
    }
    if let Some((text, bounds, source)) = tree_selection(el, 0) {
        return Some(Selection { text, bounds, source });
    }
    None
}

/// 兼容旧入口：只要文本
pub unsafe fn ax_selected_text() -> Option<String> {
    ax_selection().map(|s| s.text)
}

/// 兼容旧入口：只要矩形
pub unsafe fn ax_selection_bounds() -> Option<SelBounds> {
    ax_selection().map(|s| s.bounds)
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
