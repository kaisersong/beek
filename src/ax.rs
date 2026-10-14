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

/// 用 AXBoundsForRange 取选区真实矩形
pub unsafe fn ax_selection_bounds() -> Option<SelBounds> {
    let sys = AXUIElementCreateSystemWide();
    let app = ax_get_attr(sys, kAXFocusedApplicationAttribute).ok()? as AXUIElementRef;
    let el = ax_get_attr(app, kAXFocusedUIElementAttribute).ok()? as AXUIElementRef;
    let range = ax_get_attr(el, "AXSelectedTextMarkerRange").ok()?;
    let val = ax_param_attr(el, "AXBoundsForRange", range).ok()?;
    let mut rect = [0f64; 4];
    let ok: bool = AXValueGetValue(val as _, 2, rect.as_mut_ptr() as *mut c_void);
    if ok && rect[2] > 0. && rect[3] > 0. {
        Some(SelBounds { x: rect[0], y: rect[1], w: rect[2], h: rect[3] })
    } else { None }
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

/// URL 不弹窗
pub fn is_url(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("http://") || t.starts_with("https://") || t.starts_with("www.")
}
