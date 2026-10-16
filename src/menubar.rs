// BeeK 菜单栏模块 — NSStatusItem + Dock 显示策略
use crate::config::{self, Config};
use crate::icons;
use cocoa::base::{id, nil};
use cocoa::foundation::{NSPoint, NSString, NSSize, NSUInteger};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel, YES};
use objc::{class, msg_send, sel, sel_impl};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, OnceLock};

const POLICY_REGULAR: i64 = 0;
const POLICY_ACCESSORY: i64 = 1;

/// 「在 Dock 中显示」菜单项地址，供自检时模拟点击（0 表示尚未创建）
static TOGGLE_ITEM: AtomicUsize = AtomicUsize::new(0);

/// 主循环的事件发送端：菜单里那些「要开窗口 / 要发网络请求」的动作
/// （设置 Key、检查更新、关于）都通过它转交给主循环执行。
/// 菜单动作本身就在主线程，但需要 GPUI 上下文或后台线程的活儿不能直接干。
static UI_TX: OnceLock<mpsc::Sender<crate::UiEvent>> = OnceLock::new();

/// 由 main 在创建完通道后调用一次
pub fn set_ui_sender(tx: mpsc::Sender<crate::UiEvent>) {
    let _ = UI_TX.set(tx);
}

/// 把菜单动作转成主循环事件；通道还没就绪时只记日志（不应发生）
fn send_event(ev: crate::UiEvent) {
    match UI_TX.get() {
        Some(tx) => {
            let _ = tx.send(ev);
        }
        None => eprintln!("[beek] ⚠️ 主循环通道尚未就绪，菜单动作被丢弃"),
    }
}

/// 当前激活策略：0=Regular（有 Dock 图标）1=Accessory（仅菜单栏）
pub fn activation_policy() -> i64 {
    unsafe {
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let policy: i64 = msg_send![app, activationPolicy];
        policy
    }
}

/// 「在 Dock 中显示」菜单项的勾选状态：0=未勾选 1=已勾选
pub fn toggle_item_state() -> i64 {
    unsafe {
        let item = TOGGLE_ITEM.load(Ordering::SeqCst) as id;
        if item == nil {
            return -1;
        }
        let state: i64 = msg_send![item, state];
        state
    }
}

/// 自检用：走真实菜单项 target/action 触发一次「在 Dock 中显示」切换，返回切换后的 activationPolicy
pub fn trigger_dock_toggle() -> i64 {
    unsafe {
        let item = TOGGLE_ITEM.load(Ordering::SeqCst) as id;
        if item == nil {
            return -1;
        }
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let target: id = msg_send![item, target];
        let action: Sel = msg_send![item, action];
        let _: () = msg_send![app, sendAction: action to: target from: item];
        activation_policy()
    }
}

/// 自检用：打印菜单栏菜单的全部条目（标题 / 勾选 / 是否可用）
pub fn dump_menu() {
    unsafe {
        let item = TOGGLE_ITEM.load(Ordering::SeqCst) as id;
        if item == nil {
            eprintln!("[beek] 菜单未创建");
            return;
        }
        let menu: id = msg_send![item, menu];
        let count: usize = msg_send![menu, numberOfItems];
        eprintln!("[beek] 菜单栏菜单共 {} 项：", count);
        for i in 0..count {
            let mi: id = msg_send![menu, itemAtIndex: i];
            let title: id = msg_send![mi, title];
            let title_ptr: *const std::os::raw::c_char = msg_send![title, UTF8String];
            let title_s = if title_ptr.is_null() {
                "?".to_string()
            } else {
                std::ffi::CStr::from_ptr(title_ptr).to_string_lossy().into_owned()
            };
            let state: i64 = msg_send![mi, state];
            let enabled: i8 = msg_send![mi, isEnabled];
            // 图标也报出来：菜单项上是真挂了图（而不是只写了标题）
            let img: id = msg_send![mi, image];
            let has_img = if img == nil { "无" } else { "有" };
            eprintln!(
                "[beek]   [{}] 「{}」勾选={} 可用={} 图标={}",
                i,
                title_s,
                state,
                enabled != 0,
                has_img
            );
        }
    }
}

/// 自检用：打印 NSApp 当前所有窗口（类名 / 可见性 / 层级 / Cocoa 坐标 frame）
pub fn dump_windows(tag: &str) {
    unsafe {
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let windows: id = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        eprintln!("[beek] {}：NSApp 共 {} 个窗口", tag, count);
        for i in 0..count {
            let w: id = msg_send![windows, objectAtIndex: i];
            let frame: cocoa::foundation::NSRect = msg_send![w, frame];
            let visible: i8 = msg_send![w, isVisible];
            let level: i64 = msg_send![w, level];
            let cls: id = msg_send![w, class];
            let desc: id = msg_send![cls, description];
            let cls_ptr: *const std::os::raw::c_char = msg_send![desc, UTF8String];
            let cls_name = if cls_ptr.is_null() {
                "?".to_string()
            } else {
                std::ffi::CStr::from_ptr(cls_ptr).to_string_lossy().into_owned()
            };
            eprintln!(
                "[beek]   窗口[{}] {} visible={} level={} frame=({:.0}, {:.0}, {:.0}, {:.0})",
                i,
                cls_name,
                visible != 0,
                level,
                frame.origin.x,
                frame.origin.y,
                frame.size.width,
                frame.size.height
            );
        }
    }
}

/// 把弹窗窗口的左上角移到 Cocoa 屏幕坐标 (x, y_top)，返回是否找到并移动了窗口。
///
/// gpui 0.2.2 的 mac 平台没有移动窗口的 API：`Div::window_control_area(Drag)` 在
/// mac/window.rs 里的 `on_hit_test_window_control` 是空实现（只有 Windows 用它做 HTCAPTION），
/// 所以只能绕过 gpui，直接对 AppKit 的窗口下手。这里按宽度 + isVisible 认出「当前显示的那个」
/// 弹窗（宽度区分弹窗与图标窗口；isVisible 排除已关闭的旧弹窗，它们的 NSWindow 还在 NSApp.windows
/// 里），再用 setFrameTopLeftPoint —— 不需要任何额外权限。
///
/// 必须在主线程调用：AppKit 的窗口事务（NSWMWindowCoordinator）在别的线程上会直接让进程
/// trap 掉（实测 EXC_BREAKPOINT / SIGTRAP，见 2026-10-14 的崩溃报告）。
pub fn popup_window_move(x: f64, y_top: f64) -> bool {
    if !is_main_thread() {
        eprintln!("[beek] ⚠️ popup_window_move 只能在主线程调用，本次移动已忽略");
        debug_assert!(false, "popup_window_move 必须在主线程调用");
        return false;
    }
    unsafe {
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let windows: id = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        for i in 0..count {
            let w: id = msg_send![windows, objectAtIndex: i];
            let frame: cocoa::foundation::NSRect = msg_send![w, frame];
            let visible: i8 = msg_send![w, isVisible];
            if visible != 0 && (frame.size.width - crate::POPUP_W as f64).abs() < 1.0 {
                let p = NSPoint::new(x, y_top);
                let _: () = msg_send![w, setFrameTopLeftPoint: p];
                return true;
            }
        }
        false
    }
}

/// 当前线程是不是主线程（AppKit 的窗口操作只允许在主线程做）
fn is_main_thread() -> bool {
    extern "C" {
        fn pthread_main_np() -> i32;
    }
    unsafe { pthread_main_np() != 0 }
}

/// 当前可见的翻译弹窗个数（按弹窗宽度识别，自检用）。
/// 高度是按内容变的，所以只能靠宽度区分（图标窗口只有 32pt 宽）。
pub fn popup_window_count() -> usize {
    unsafe {
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let windows: id = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        let mut found = 0usize;
        for i in 0..count {
            let w: id = msg_send![windows, objectAtIndex: i];
            let frame: cocoa::foundation::NSRect = msg_send![w, frame];
            let visible: i8 = msg_send![w, isVisible];
            if visible != 0 && (frame.size.width - crate::POPUP_W as f64).abs() < 1.0 {
                found += 1;
            }
        }
        found
    }
}

/// 切换 Dock 图标显示。Accessory = 只保留菜单栏图标，不在 Dock 显示。
pub fn set_dock_visible(visible: bool) {
    unsafe {
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let policy = if visible { POLICY_REGULAR } else { POLICY_ACCESSORY };
        let _: () = msg_send![app, setActivationPolicy: policy];
        let cur: i64 = msg_send![app, activationPolicy];
        eprintln!(
            "[beek] Dock 图标：{}（activationPolicy={}，0=显示 1=隐藏）",
            if visible { "显示" } else { "隐藏" },
            cur
        );
    }
}

unsafe fn nsstr(s: &str) -> id {
    NSString::alloc(nil).init_str(s)
}

// ---------- 动作实现：菜单项（objc target）与 gpui action 共用同一份逻辑 ----------

/// 切换「在 Dock 中显示」，返回切换后的值。
pub fn flip_show_in_dock() -> bool {
    let show = !config::load().show_in_dock;
    set_dock_visible(show);
    // 读-改-写：不能整份覆盖，否则会把 providers 里配好的 key 抹掉
    config::update(|c| c.show_in_dock = show);
    show
}

pub fn flip_copy_to_popup() -> bool {
    let on = !config::load().copy_to_popup;
    config::update(|c| c.copy_to_popup = on);
    eprintln!("[beek] 复制即弹按钮：{}", if on { "开" } else { "关" });
    on
}

pub fn flip_clipboard_fallback() -> bool {
    let on = !config::load().clipboard_fallback;
    config::update(|c| c.clipboard_fallback = on);
    eprintln!("[beek] 剪贴板兜底取词：{}", if on { "开" } else { "关" });
    on
}

/// 直接打开「辅助功能」授权面板，方便用户勾选 BeeK
pub fn open_accessibility_panel() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .status();
}

/// 打开 README 的「获取 API Key」章节（纯跳转，不需要经过主循环）
pub fn open_readme_key_url() {
    crate::update::open_url(crate::update::README_KEY_URL);
}

/// 重启：KeepAlive=true，正常退出会被 launchd 立刻重新拉起（用于让辅助功能授权生效）
pub fn restart_now() -> ! {
    std::process::exit(0)
}

/// 退出：服务由 launchd KeepAlive 托管，先 bootout 再退出，避免被立即重启。
pub fn quit_now() -> ! {
    let _ = std::process::Command::new("sh")
        .arg("-c")
        .arg("launchctl bootout gui/$(id -u)/com.kaisersong.beek 2>/dev/null")
        .status();
    std::process::exit(0)
}

// ---------- 菜单项挂的 objc target ----------

extern "C" fn toggle_dock(_this: &Object, _sel: Sel, sender: id) {
    let on = flip_show_in_dock();
    unsafe {
        let _: () = msg_send![sender, setState: if on { 1i64 } else { 0i64 }];
    }
}

extern "C" fn open_accessibility_settings(_this: &Object, _sel: Sel, _sender: id) {
    open_accessibility_panel();
}

extern "C" fn toggle_copy_to_popup(_this: &Object, _sel: Sel, sender: id) {
    let on = flip_copy_to_popup();
    unsafe {
        let _: () = msg_send![sender, setState: if on { 1i64 } else { 0i64 }];
    }
}

extern "C" fn toggle_clipboard_fallback(_this: &Object, _sel: Sel, sender: id) {
    let on = flip_clipboard_fallback();
    unsafe {
        let _: () = msg_send![sender, setState: if on { 1i64 } else { 0i64 }];
    }
}

extern "C" fn open_key_settings(_this: &Object, _sel: Sel, _sender: id) {
    send_event(crate::UiEvent::OpenKeySettings);
}

extern "C" fn open_readme_key(_this: &Object, _sel: Sel, _sender: id) {
    open_readme_key_url();
}

extern "C" fn check_update(_this: &Object, _sel: Sel, _sender: id) {
    send_event(crate::UiEvent::CheckUpdate);
}

extern "C" fn open_about(_this: &Object, _sel: Sel, _sender: id) {
    send_event(crate::UiEvent::OpenAbout);
}

extern "C" fn restart_app(_this: &Object, _sel: Sel, _sender: id) {
    restart_now();
}

extern "C" fn quit_app(_this: &Object, _sel: Sel, _sender: id) {
    quit_now();
}

/// 状态项与其菜单的地址，供「程序化弹出菜单」使用（Dock 图标被点/重新打开时）。
static STATUS_ITEM: AtomicUsize = AtomicUsize::new(0);
static STATUS_MENU: AtomicUsize = AtomicUsize::new(0);

/// 弹出状态项菜单（等价于用鼠标点菜单栏图标）。
///
/// 用途：这个 app 没有任何窗口，所以点 Dock 图标时 macOS 发的是「重新打开」
/// （applicationShouldHandleReopen）；不处理的话用户看起来就是「点了没反应」。
pub fn popup_status_menu() -> bool {
    unsafe {
        let item = STATUS_ITEM.load(Ordering::SeqCst) as id;
        let menu = STATUS_MENU.load(Ordering::SeqCst) as id;
        if item == nil || menu == nil {
            return false;
        }
        // popUpStatusItemMenu: 虽然被标记为 deprecated，但仍是程序化弹出状态项菜单的做法
        let _: () = msg_send![item, popUpStatusItemMenu: menu];
        true
    }
}

/// 给 gpui 建好的菜单栏补一个「编辑」菜单。
///
/// 为什么这一项不用 gpui 的菜单模型：gpui 的 delegate 自己实现了 copy:/paste:，
/// 用 gpui 的 os_action 建出来的项，点下去是发给 delegate（再派发成 gpui action），
/// **不会**走到文本框 —— 也就是「设置 API Key」里的输入框按 ⌘V 没反应。
/// 这里手搓的项 target 为空，动作沿响应链走，第一响应者（field editor）才收得到，
/// ⌘X / ⌘C / ⌘V / ⌘A 才是正常的。
pub fn install_edit_menu() {
    unsafe {
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let main: id = msg_send![app, mainMenu];
        if main == nil {
            eprintln!("[beek] ⚠️ 主菜单还不存在，编辑菜单没装上");
            return;
        }
        let count: usize = msg_send![main, numberOfItems];
        for i in 0..count {
            let it: id = msg_send![main, itemAtIndex: i];
            if item_title(it) == "编辑" {
                return; // 已经装过（gpui 重建主菜单后我们会再调一次）
            }
        }

        let item_alloc: id = msg_send![class!(NSMenuItem), alloc];
        let item: id = msg_send![item_alloc, init];
        let sub_alloc: id = msg_send![class!(NSMenu), alloc];
        let sub: id = msg_send![sub_alloc, init];
        // 标题两边都要写：父项没有标题时，菜单栏会把它显示成类名（NSMenuItem）
        let _: () = msg_send![sub, setTitle: nsstr("编辑")];
        let _: () = msg_send![item, setTitle: nsstr("编辑")];
        add_item(sub, "剪切", sel!(cut:), "x", nil);
        add_item(sub, "复制", sel!(copy:), "c", nil);
        add_item(sub, "粘贴", sel!(paste:), "v", nil);
        add_item(sub, "全选", sel!(selectAll:), "a", nil);
        let _: () = msg_send![item, setSubmenu: sub];
        let _: () = msg_send![main, addItem: item];
        eprintln!("[beek] 编辑菜单已挂上（target 为空 → 走响应链，对话框输入框的 ⌘X/⌘C/⌘V/⌘A 可用）");
    }
}

/// 取证：打印 NSApp 的主菜单，以及问 gpui 的 delegate 要一份 Dock 菜单。
/// Dock 菜单在 macOS 上就是通过 `applicationDockMenu:` 这个委托方法要的，
/// 所以这里不看我们设了什么，而是看 delegate 实际返回什么。
pub fn dump_app_menus() {
    unsafe {
        let app: id = msg_send![class!(NSApplication), sharedApplication];
        let main: id = msg_send![app, mainMenu];
        dump_menu_desc("主菜单", main);

        let delegate: id = msg_send![app, delegate];
        if delegate == nil {
            eprintln!("[beek] NSApp.delegate 为空，Dock 菜单无从谈起");
            return;
        }
        let sel_dock = sel!(applicationDockMenu:);
        let responds: i8 = msg_send![delegate, respondsToSelector: sel_dock];
        if responds == 0 {
            eprintln!("[beek] delegate 没实现 applicationDockMenu:（Dock 右键会退回系统默认菜单）");
            return;
        }
        let dock: id = msg_send![delegate, applicationDockMenu: app];
        dump_menu_desc("Dock 右键（delegate 实际返回的）", dock);
    }
}

/// 创建菜单栏状态图标和菜单。handler/status item 由系统持有，永不释放。

// ---------- 菜单构造：状态项 / Dock 右键 / 主菜单共用 ----------

unsafe fn item_title(item: id) -> String {
    let t: id = msg_send![item, title];
    if t == nil {
        return String::new();
    }
    let ptr: *const std::os::raw::c_char = msg_send![t, UTF8String];
    if ptr.is_null() {
        String::new()
    } else {
        std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

/// 打印一个菜单有哪些项（含子菜单里的项）。取证用。
pub fn dump_menu_desc(tag: &str, menu: id) {
    unsafe {
        if menu == nil {
            eprintln!("[beek] {} 菜单：未创建", tag);
            return;
        }
        let count: usize = msg_send![menu, numberOfItems];
        eprintln!("[beek] {} 菜单共 {} 项：", tag, count);
        for i in 0..count {
            let mi: id = msg_send![menu, itemAtIndex: i];
            let title = item_title(mi);
            let sub: id = msg_send![mi, submenu];
            if sub == nil {
                eprintln!("[beek]   [{}] 「{}」", i, title);
            } else {
                let n: usize = msg_send![sub, numberOfItems];
                let mut names: Vec<String> = Vec::new();
                for j in 0..n {
                    let sj: id = msg_send![sub, itemAtIndex: j];
                    let st = item_title(sj);
                    names.push(if st.is_empty() { "—（分隔线）".to_string() } else { st });
                }
                eprintln!(
                    "[beek]   [{}] 「{}」→ {} 项：{}",
                    i,
                    title,
                    n,
                    names.join(" / ")
                );
            }
        }
    }
}

/// 往菜单里挂一项。target 传 nil 表示走响应链：隐藏窗口、剪切复制粘贴、全选
/// 这类标准动作由 AppKit 自己处理，不需要我们实现。
unsafe fn add_item(menu: id, title: &str, action: Sel, key: &str, target: id) -> id {
    let alloc: id = msg_send![class!(NSMenuItem), alloc];
    let mi: id = msg_send![alloc,
        initWithTitle: nsstr(title)
        action: action
        keyEquivalent: nsstr(key)];
    if target != nil {
        let _: () = msg_send![mi, setTarget: target];
    }
    let _: () = msg_send![menu, addItem: mi];
    mi
}

unsafe fn add_separator(menu: id) {
    let sep: id = msg_send![class!(NSMenuItem), separatorItem];
    let _: () = msg_send![menu, addItem: sep];
}

/// 建一套 BeeK 的动作菜单，返回（菜单, 「在 Dock 中显示」那一项）。
unsafe fn build_menu(cfg: &Config, handler: id) -> (id, id) {
    let menu_alloc: id = msg_send![class!(NSMenu), alloc];
    let menu: id = msg_send![menu_alloc, init];

    // 第一项只显示版本，不可点
    let title = add_item(
        menu,
        &format!("BeeK 划词翻译 v{}", crate::update::CURRENT_VERSION),
        sel!(noop:),
        "",
        nil,
    );
    let _: () = msg_send![title, setEnabled: false];

    add_item(menu, "开启取词权限（辅助功能）…", sel!(openAccessibilitySettings:), "", handler);

    let toggle = add_item(menu, "在 Dock 中显示", sel!(toggleDock:), "", handler);
    let _: () = msg_send![toggle, setState: if cfg.show_in_dock { 1i64 } else { 0i64 }];

    // 复制即弹按钮：你自己按 ⌘C 时在鼠标旁弹小蜜蜂（不合成输入、不依赖 AX，最可靠）
    let c2p = add_item(menu, "复制即弹按钮（自己按 ⌘C）", sel!(toggleCopyToPopup:), "", handler);
    let _: () = msg_send![c2p, setState: if cfg.copy_to_popup { 1i64 } else { 0i64 }];

    // 剪贴板兜底：AX 取不到选区时，模拟一次 ⌘C 读剪贴板（Safari/Muse/Grok Bot 这类应用必须开）
    let clip = add_item(menu, "剪贴板兜底取词", sel!(toggleClipboardFallback:), "", handler);
    let _: () = msg_send![clip, setState: if cfg.clipboard_fallback { 1i64 } else { 0i64 }];

    add_separator(menu);

    // 设置 API Key：弹出的对话框由主循环用 AppKit 的 NSAlert 呈现
    add_item(menu, "设置 API Key…", sel!(openKeySettings:), "", handler);

    // 「如何获取 API Key…」：带一个圆圈 i 图标，点开 README 对应章节
    let help = add_item(menu, "如何获取 API Key…", sel!(openReadmeKey:), "", handler);
    let png = icons::CIRCLE_I_PNG;
    let data: id = msg_send![class!(NSData),
        dataWithBytes: png.as_ptr() as *const std::os::raw::c_void
        length: png.len() as NSUInteger];
    let img_alloc: id = msg_send![class!(NSImage), alloc];
    let circle: id = msg_send![img_alloc, initWithData: data];
    if circle == nil {
        eprintln!("[beek] ⚠️ 圆圈 i 图标解码失败，菜单项将没有图标");
    } else {
        let sz = NSSize::new(icons::MENU_ICON_PT, icons::MENU_ICON_PT);
        let _: () = msg_send![circle, setSize: sz];
        // template：交给 AppKit 按菜单明暗自动反色
        let _: () = msg_send![circle, setTemplate: YES];
        let _: () = msg_send![help, setImage: circle];
    }

    add_separator(menu);
    add_item(menu, "检查更新…", sel!(checkUpdate:), "", handler);
    add_item(menu, "关于 BeeK", sel!(openAbout:), "", handler);
    add_separator(menu);
    add_item(menu, "重新启动 BeeK", sel!(restartApp:), "", handler);
    add_item(menu, "退出 BeeK", sel!(quitApp:), "q", handler);

    (menu, toggle)
}

pub fn setup_status_item(cfg: &Config) {
    unsafe {
        let superclass = Class::get("NSObject").unwrap();
        let mut decl = ClassDecl::new("BeeKMenuHandler", superclass).unwrap();
        decl.add_method(sel!(toggleDock:), toggle_dock as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(openAccessibilitySettings:), open_accessibility_settings as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(quitApp:), quit_app as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(restartApp:), restart_app as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(toggleClipboardFallback:), toggle_clipboard_fallback as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(toggleCopyToPopup:), toggle_copy_to_popup as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(openKeySettings:), open_key_settings as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(openReadmeKey:), open_readme_key as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(checkUpdate:), check_update as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(openAbout:), open_about as extern "C" fn(&Object, Sel, id));
        let cls = decl.register();
        let handler: id = msg_send![cls, new];

        let status_bar: id = msg_send![class!(NSStatusBar), systemStatusBar];
        let item: id = msg_send![status_bar, statusItemWithLength: -1.0f64];
        // NSStatusBar 返回的对象可能被自动释放，显式 retain 保证菜单栏图标常驻。
        let _: id = msg_send![item, retain];
        let button: id = msg_send![item, button];
        // 菜单栏图标：内嵌的 36px @2x template 图（纯黑 + alpha），
        // setTemplate 之后 AppKit 会按菜单栏明暗自动反色，无需两套素材。
        let png = icons::MENUBAR_TEMPLATE_PNG;
        let data: id = msg_send![class!(NSData),
            dataWithBytes: png.as_ptr() as *const std::os::raw::c_void
            length: png.len() as NSUInteger];
        let img_alloc: id = msg_send![class!(NSImage), alloc];
        let img: id = msg_send![img_alloc, initWithData: data];
        if img == nil {
            eprintln!("[beek] ⚠️ 菜单栏图标解码失败，回退为 emoji");
            let _: () = msg_send![button, setTitle: nsstr("🐝")];
        } else {
            let sz = NSSize::new(
                icons::MENUBAR_ICON_PT as _,
                icons::MENUBAR_ICON_PT as _,
            );
            let _: () = msg_send![img, setSize: sz];
            let _: () = msg_send![img, setTemplate: YES];
            let _: () = msg_send![button, setImage: img];
            let _: () = msg_send![button, setImagePosition: 1i64]; // NSImageOnly
            let _: () = msg_send![button, setTitle: nsstr("")];
        }

        // 状态项自己的菜单（唯一手搓的一份，因为它不是「应用级菜单」，
        // gpui 的菜单 API 管不到菜单栏图标）。带勾选态和圆圈 i 图标，没法用 gpui 的模型表达。
        let (menu, toggle) = build_menu(cfg, handler);
        let _: () = msg_send![item, setMenu: menu];
        TOGGLE_ITEM.store(toggle as usize, Ordering::SeqCst);
        STATUS_ITEM.store(item as usize, Ordering::SeqCst);
        STATUS_MENU.store(menu as usize, Ordering::SeqCst);

        // 应用级菜单（主菜单 / Dock 右键菜单）不在这里建：macOS 要 Dock 菜单是走
        // applicationDockMenu: 这个**委托方法**，而 delegate 是 gpui 自己实现的，
        // 我们手写挂在 NSApp.dockMenu 上的那份会被它挡住。改用 gpui 的
        // set_menus / set_dock_menu（见 main.rs 的 setup_app_menus）。
        dump_menu_desc("菜单栏图标菜单", menu);

        // 自检：状态栏按钮应挂在窗口上且可见，可用于确认菜单栏图标真的显示了
        let btn_window: id = msg_send![button, window];
        if btn_window == nil {
            eprintln!("[beek] ⚠️ 菜单栏图标创建失败：按钮未挂到状态栏窗口");
        } else {
            let frame: cocoa::foundation::NSRect = msg_send![btn_window, frame];
            let visible: i8 = msg_send![btn_window, isVisible];
            // 回读按钮实际持有的图像，确认 setImage 真的生效（不依赖肉眼）
            let shown_img: id = msg_send![button, image];
            let img_desc = if shown_img == nil {
                "无（已回退 emoji）".to_string()
            } else {
                let s: NSSize = msg_send![shown_img, size];
                let is_tpl: objc::runtime::BOOL = msg_send![shown_img, isTemplate];
                format!("{:.0}x{:.0}pt template={}", s.width, s.height, is_tpl)
            };
            let btn_title: id = msg_send![button, title];
            let title_ptr: *const std::os::raw::c_char = msg_send![btn_title, UTF8String];
            let title_s = if title_ptr.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(title_ptr).to_string_lossy().into_owned()
            };
            eprintln!(
                "[beek] 菜单栏图标已就绪：visible={} frame=({:.0}, {:.0}, {:.0}, {:.0}) 图像={} 标题={:?}",
                visible != 0,
                frame.origin.x,
                frame.origin.y,
                frame.size.width,
                frame.size.height,
                img_desc,
                title_s
            );

            // 状态栏布局是异步的，2 秒后再确认一次真实位置
            let addr = btn_window as usize;
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let w = addr as id;
                unsafe {
                    let frame: cocoa::foundation::NSRect = msg_send![w, frame];
                    let visible: i8 = msg_send![w, isVisible];
                    eprintln!(
                        "[beek] 菜单栏图标（2s 后）：visible={} frame=({:.0}, {:.0}, {:.0}, {:.0})",
                        visible != 0,
                        frame.origin.x,
                        frame.origin.y,
                        frame.size.width,
                        frame.size.height
                    );
                }
            });
        }
    }
}
