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

/// 选区矩形 (x, y, w, h)，Cocoa 屏幕坐标系
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

/// 取选区真实矩形：焦点元素优先，必要时沿父链上溯
pub unsafe fn ax_selection_bounds() -> Option<SelBounds> {
    let el = focused_element()?;
    let mut cur = el;
    for _ in 0..5 {
        if let Some(b) = bounds_routes(cur) {
            return Some(b);
        }
        match ax_get_attr(cur, kAXParentAttribute) {
            Ok(p) => cur = p as AXUIElementRef,
            Err(_) => break,
        }
    }
    let fb = element_frame(el);
    if debug_on() {
        eprintln!("[beek] 选区矩形：三条 range 路线都失败，兜底元素矩形 = {:?}", fb);
    }
    fb
}

unsafe fn get_role(el: AXUIElementRef) -> Option<String> {
    let v = ax_get_attr(el, kAXRoleAttribute).ok()?;
    cfstring_to_string(v)
}

/// 通用树搜索：BFS 找第一个有选中文本的元素
unsafe fn tree_search_text(el: AXUIElementRef, depth: u32) -> Option<String> {
    if depth > 6 { return None; }
    if let Some(t) = try_get_text(el) { return Some(t); }
    if let Ok(range) = ax_get_attr(el, "AXSelectedTextMarkerRange") {
        if let Ok(v) = ax_param_attr(el, "AXStringForRange", range) {
            if let Some(t) = cfstring_to_string(v) { return Some(t); }
        }
    }
    if let Ok(children) = ax_get_attr(el, kAXChildrenAttribute) {
        let arr = core_foundation::array::CFArray::<*const c_void>::wrap_under_get_rule(children as _);
        for i in 0..arr.len().min(50) {
            let child = *arr.get(i).unwrap() as AXUIElementRef;
            if let Some(t) = tree_search_text(child, depth + 1) { return Some(t); }
        }
    }
    None
}

/// 主入口：取当前选中文本，含多层 fallback
pub unsafe fn ax_selected_text() -> Option<String> {
    let sys = AXUIElementCreateSystemWide();
    AXUIElementSetMessagingTimeout(sys, 0.05);
    let app = ax_get_attr(sys, kAXFocusedApplicationAttribute).ok()? as AXUIElementRef;
    if let Ok(el) = ax_get_attr(app, kAXFocusedUIElementAttribute).map(|v| v as AXUIElementRef) {
        if let Some(t) = try_get_text(el) { return Some(t); }
        if let Ok(range) = ax_get_attr(el, "AXSelectedTextMarkerRange") {
            if let Ok(v) = ax_param_attr(el, "AXStringForRange", range) {
                if let Some(t) = cfstring_to_string(v) { return Some(t); }
            }
        }
        let mut cur = el;
        for _ in 0..5 {
            if let Ok(p) = ax_get_attr(cur, kAXParentAttribute).map(|v| v as AXUIElementRef) {
                if let Some(t) = try_get_text(p) { return Some(t); }
                cur = p;
            } else { break; }
        }
        if let Ok(win) = ax_get_attr(app, kAXFocusedWindowAttribute).map(|v| v as AXUIElementRef) {
            if let Some(t) = tree_search_text(win, 0) { return Some(t); }
        }
    }
    if let Ok(win) = ax_get_attr(app, kAXFocusedWindowAttribute).map(|v| v as AXUIElementRef) {
        if let Some(t) = find_webarea_text(win, 0) { return Some(t); }
    }
    if let Ok(v) = ax_get_attr(app, kAXSelectedTextAttribute) {
        if let Some(t) = cfstring_to_string(v) { return Some(t); }
    }
    None
}

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
