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

/// 子树里找「自己有选中文本」的元素，并把它的文本 + 矩形一起带回来。
/// 从焦点窗口搜索时树可能很大，用节点预算兜住（选区只在「要显示图标」时查一次）。
unsafe fn tree_selection(
    el: AXUIElementRef,
    depth: u32,
    budget: &mut u32,
) -> Option<(String, SelBounds, &'static str)> {
    if depth > 8 || *budget == 0 {
        return None;
    }
    *budget -= 1;
    if let Some(hit) = element_selection(el) {
        return Some((hit.0, hit.1, "tree"));
    }
    if let Ok(children) = ax_get_attr(el, kAXChildrenAttribute) {
        let arr = core_foundation::array::CFArray::<*const c_void>::wrap_under_get_rule(children as _);
        for i in 0..arr.len().min(50) {
            let child = *arr.get(i).unwrap() as AXUIElementRef;
            if let Some(hit) = tree_selection(child, depth + 1, budget) {
                return Some((hit.0, hit.1, "tree"));
            }
        }
    }
    None
}

/// 找 AXWebArea 上的选区。
/// Safari / Chromium（Muse、Grok Bot 这类）把选区文本挂在 AXWebArea 上，
/// 而不是焦点元素上 —— 丢了这条路，这些应用就完全弹不出图标（实测回归）。
unsafe fn webarea_selection(
    el: AXUIElementRef,
    depth: u32,
    budget: &mut u32,
) -> Option<(String, SelBounds, &'static str)> {
    if depth > 8 || *budget == 0 {
        return None;
    }
    *budget -= 1;
    if get_role(el).map(|r| r == "AXWebArea").unwrap_or(false) {
        if let Some((text, bounds, _)) = element_selection(el) {
            return Some((text, bounds, "webarea"));
        }
    }
    if let Ok(children) = ax_get_attr(el, kAXChildrenAttribute) {
        let arr = core_foundation::array::CFArray::<*const c_void>::wrap_under_get_rule(children as _);
        for i in 0..arr.len().min(50) {
            let child = *arr.get(i).unwrap() as AXUIElementRef;
            if let Some(hit) = webarea_selection(child, depth + 1, budget) {
                return Some(hit);
            }
        }
    }
    None
}

/// 取当前选区（文本 + 矩形，同一个元素）
///
/// 注意：**不能**在拿不到「app 级焦点元素」时直接返回。
/// 实测（探针 BEEK_AX_PROBE_PID=2238 打在 Safari 上）Safari 不提供
/// app 级 AXFocusedUIElement（返回 nil），于是只剩「从焦点窗口搜 AXWebArea」这条能走的路。
/// 早期版本就是因为在第一步就 `?` 掉，导致 Safari / Chromium 类应用（Muse、Grok Bot）
/// 完全弹不出图标。
pub unsafe fn ax_selection() -> Option<Selection> {
    let sys = AXUIElementCreateSystemWide();
    AXUIElementSetMessagingTimeout(sys, 0.5);
    let app = match ax_get_attr(sys, kAXFocusedApplicationAttribute) {
        Ok(a) => a as AXUIElementRef,
        Err(_) => return None,
    };

    // 1) 焦点元素自己 → 沿父链最多 4 层 → 焦点元素子树（拿得到焦点元素时才有这一步）
    if let Ok(el) = ax_get_attr(app, kAXFocusedUIElementAttribute) {
        let el = el as AXUIElementRef;
        AXUIElementSetMessagingTimeout(el, 0.5);
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
        let mut budget: u32 = 400;
        if let Some(hit) = tree_selection(el, 0, &mut budget) {
            return Some(Selection { text: hit.0, bounds: hit.1, source: hit.2 });
        }
    }

    // 2) 焦点窗口子树：优先 AXWebArea（Safari / Chromium 的选区挂在这里），再退到任意有选区的节点
    if let Ok(win) = ax_get_attr(app, kAXFocusedWindowAttribute) {
        let win = win as AXUIElementRef;
        let mut budget: u32 = 800;
        if let Some(hit) = webarea_selection(win, 0, &mut budget) {
            return Some(Selection { text: hit.0, bounds: hit.1, source: hit.2 });
        }
        let mut budget: u32 = 800;
        if let Some(hit) = tree_selection(win, 0, &mut budget) {
            return Some(Selection { text: hit.0, bounds: hit.1, source: hit.2 });
        }
    }

    // 3) app 自己的 AXSelectedText（没有矩形，位置交给鼠标锚点兜底）
    if let Ok(v) = ax_get_attr(app, kAXSelectedTextAttribute) {
        if let Some(text) = cfstring_to_string(v) {
            return Some(Selection {
                text,
                bounds: SelBounds { x: 0., y: 0., w: 0., h: 0. },
                source: "app",
            });
        }
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
