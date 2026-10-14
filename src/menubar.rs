// BeeK 菜单栏模块 — NSStatusItem + Dock 显示策略
use crate::config::{self, Config};
use crate::icons;
use cocoa::base::{id, nil};
use cocoa::foundation::{NSString, NSSize, NSUInteger};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel, YES};
use objc::{class, msg_send, sel, sel_impl};
use std::sync::atomic::{AtomicUsize, Ordering};

const POLICY_REGULAR: i64 = 0;
const POLICY_ACCESSORY: i64 = 1;

/// 「在 Dock 中显示」菜单项地址，供自检时模拟点击（0 表示尚未创建）
static TOGGLE_ITEM: AtomicUsize = AtomicUsize::new(0);

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
            eprintln!(
                "[beek]   [{}] 「{}」勾选={} 可用={}",
                i,
                title_s,
                state,
                enabled != 0
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

/// 当前可见的翻译弹窗个数（按弹窗尺寸识别，自检用）
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
            if visible != 0
                && (frame.size.width - crate::POPUP_W as f64).abs() < 1.0
                && (frame.size.height - crate::POPUP_H as f64).abs() < 1.0
            {
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

extern "C" fn toggle_dock(_this: &Object, _sel: Sel, sender: id) {
    unsafe {
        let state: i64 = msg_send![sender, state];
        let show = state == 0;
        let _: () = msg_send![sender, setState: if show { 1i64 } else { 0i64 }];
        set_dock_visible(show);
        config::save(&Config { show_in_dock: show });
    }
}

extern "C" fn open_accessibility_settings(_this: &Object, _sel: Sel, _sender: id) {
    // 直接打开「辅助功能」授权面板，方便用户勾选 BeeK
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .status();
}

extern "C" fn restart_app(_this: &Object, _sel: Sel, _sender: id) {
    // KeepAlive=true，正常退出会被 launchd 立刻重新拉起，等价于重启（用于让辅助功能授权生效）
    std::process::exit(0);
}

extern "C" fn quit_app(_this: &Object, _sel: Sel, _sender: id) {
    // 服务由 launchd KeepAlive 托管，先 bootout 再退出，避免被立即重启。
    let _ = std::process::Command::new("sh")
        .arg("-c")
        .arg("launchctl bootout gui/$(id -u)/com.kaisersong.beek 2>/dev/null")
        .status();
    std::process::exit(0);
}

/// 创建菜单栏状态图标和菜单。handler/status item 由系统持有，永不释放。
pub fn setup_status_item(cfg: &Config) {
    unsafe {
        let superclass = Class::get("NSObject").unwrap();
        let mut decl = ClassDecl::new("BeeKMenuHandler", superclass).unwrap();
        decl.add_method(sel!(toggleDock:), toggle_dock as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(openAccessibilitySettings:), open_accessibility_settings as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(quitApp:), quit_app as extern "C" fn(&Object, Sel, id));
        decl.add_method(sel!(restartApp:), restart_app as extern "C" fn(&Object, Sel, id));
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

        let menu_alloc: id = msg_send![class!(NSMenu), alloc];
        let menu: id = msg_send![menu_alloc, init];

        let title_alloc: id = msg_send![class!(NSMenuItem), alloc];
        let title: id = msg_send![title_alloc,
            initWithTitle: nsstr("BeeK 划词翻译")
            action: sel!(noop:)
            keyEquivalent: nsstr("")];
        let _: () = msg_send![title, setEnabled: false];
        let _: () = msg_send![menu, addItem: title];

        let ax_alloc: id = msg_send![class!(NSMenuItem), alloc];
        let ax_item: id = msg_send![ax_alloc,
            initWithTitle: nsstr("开启取词权限（辅助功能）…")
            action: sel!(openAccessibilitySettings:)
            keyEquivalent: nsstr("")];
        let _: () = msg_send![ax_item, setTarget: handler];
        let _: () = msg_send![menu, addItem: ax_item];

        let toggle_alloc: id = msg_send![class!(NSMenuItem), alloc];
        let toggle: id = msg_send![toggle_alloc,
            initWithTitle: nsstr("在 Dock 中显示")
            action: sel!(toggleDock:)
            keyEquivalent: nsstr("")];
        let _: () = msg_send![toggle, setTarget: handler];
        let _: () = msg_send![toggle, setState: if cfg.show_in_dock { 1i64 } else { 0i64 }];
        let _: () = msg_send![menu, addItem: toggle];
        TOGGLE_ITEM.store(toggle as usize, Ordering::SeqCst);

        let sep: id = msg_send![class!(NSMenuItem), separatorItem];
        let _: () = msg_send![menu, addItem: sep];

        let restart_alloc: id = msg_send![class!(NSMenuItem), alloc];
        let restart: id = msg_send![restart_alloc,
            initWithTitle: nsstr("重新启动 BeeK")
            action: sel!(restartApp:)
            keyEquivalent: nsstr("")];
        let _: () = msg_send![restart, setTarget: handler];
        let _: () = msg_send![menu, addItem: restart];

        let quit_alloc: id = msg_send![class!(NSMenuItem), alloc];
        let quit: id = msg_send![quit_alloc,
            initWithTitle: nsstr("退出 BeeK")
            action: sel!(quitApp:)
            keyEquivalent: nsstr("q")];
        let _: () = msg_send![quit, setTarget: handler];
        let _: () = msg_send![menu, addItem: quit];

        let _: () = msg_send![item, setMenu: menu];

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
