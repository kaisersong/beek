// BeeK 菜单栏模块 — NSStatusItem + Dock 显示策略
use crate::config::{self, Config};
use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};

const POLICY_REGULAR: i64 = 0;
const POLICY_ACCESSORY: i64 = 1;

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
        decl.add_method(sel!(quitApp:), quit_app as extern "C" fn(&Object, Sel, id));
        let cls = decl.register();
        let handler: id = msg_send![cls, new];

        let status_bar: id = msg_send![class!(NSStatusBar), systemStatusBar];
        let item: id = msg_send![status_bar, statusItemWithLength: -1.0f64];
        // NSStatusBar 返回的对象可能被自动释放，显式 retain 保证菜单栏图标常驻。
        let _: id = msg_send![item, retain];
        let button: id = msg_send![item, button];
        let _: () = msg_send![button, setTitle: nsstr("🐝")];

        let menu_alloc: id = msg_send![class!(NSMenu), alloc];
        let menu: id = msg_send![menu_alloc, init];

        let title_alloc: id = msg_send![class!(NSMenuItem), alloc];
        let title: id = msg_send![title_alloc,
            initWithTitle: nsstr("BeeK 划词翻译")
            action: sel!(noop:)
            keyEquivalent: nsstr("")];
        let _: () = msg_send![title, setEnabled: false];
        let _: () = msg_send![menu, addItem: title];

        let toggle_alloc: id = msg_send![class!(NSMenuItem), alloc];
        let toggle: id = msg_send![toggle_alloc,
            initWithTitle: nsstr("在 Dock 中显示")
            action: sel!(toggleDock:)
            keyEquivalent: nsstr("")];
        let _: () = msg_send![toggle, setTarget: handler];
        let _: () = msg_send![toggle, setState: if cfg.show_in_dock { 1i64 } else { 0i64 }];
        let _: () = msg_send![menu, addItem: toggle];

        let sep: id = msg_send![class!(NSMenuItem), separatorItem];
        let _: () = msg_send![menu, addItem: sep];

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
            eprintln!(
                "[beek] 菜单栏图标已就绪：visible={} frame=({:.0}, {:.0}, {:.0}, {:.0})",
                visible != 0,
                frame.origin.x,
                frame.origin.y,
                frame.size.width,
                frame.size.height
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
