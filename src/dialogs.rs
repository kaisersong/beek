// BeeK 原生对话框 — 设置 API Key / 关于 / 更新提示
//
// 为什么用 AppKit 的 NSAlert 而不是 GPUI 窗口：gpui 0.2.2 没有文本输入控件
// （TextInput 在 Zed 的 ui crate 里，本项目没引这个依赖），而配置 Key 必须有输入框。
// NSAlert + NSTextField 是系统标准做法，样式也跟菜单栏 app 的定位一致。
//
// 线程约束：全部只能在主线程调用（菜单动作和 GPUI 主循环都在主线程，
// 后台线程只能把结果通过 UiEvent 送回主循环再弹）。
use cocoa::base::{id, nil};
use cocoa::foundation::{NSInteger, NSPoint, NSRect, NSSize, NSString};
use objc::runtime::{NO, YES};
use objc::{class, msg_send, sel, sel_impl};

fn nsstr(s: &str) -> id {
    unsafe { NSString::alloc(nil).init_str(s) }
}

/// 自检模式：不真的弹窗、不阻塞，直接返回「第一个按钮」。
/// 这样 BEEK_SELFTEST 能把「菜单 → 主循环 → 对话框 → 落盘」这条链路跑通，
/// 而不需要有人坐在屏幕前点按钮。生产（没设这个环境变量）时行为完全不变。
fn selftest_mode() -> bool {
    std::env::var("BEEK_SELFTEST").is_ok()
}

unsafe fn nsstring_value(v: id) -> String {
    if v == nil {
        return String::new();
    }
    let ptr: *const std::os::raw::c_char = msg_send![v, UTF8String];
    if ptr.is_null() {
        String::new()
    } else {
        std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

/// 弹一个原生提示框，返回被点按钮的下标（0 = 第一个按钮）。
/// NSAlert 的返回值从 1000 起算（NSAlertFirstButtonReturn）。
pub fn alert(title: &str, body: &str, buttons: &[&str]) -> usize {
    if selftest_mode() {
        eprintln!(
            "[自检] 对话框「{}」：{}｜按钮 {:?} → 自检模式返回第 1 个",
            title,
            body.replace('\n', " / "),
            buttons
        );
        return 0;
    }
    unsafe {
        let alert: id = msg_send![class!(NSAlert), new];
        let _: () = msg_send![alert, setMessageText: nsstr(title)];
        if !body.is_empty() {
            let _: () = msg_send![alert, setInformativeText: nsstr(body)];
        }
        let _: () = msg_send![alert, setAlertStyle: 1i64]; // NSAlertStyleInformational
        for b in buttons {
            let _: id = msg_send![alert, addButtonWithTitle: nsstr(b)];
        }
        let resp: NSInteger = msg_send![alert, runModal];
        (resp - 1000).max(0) as usize
    }
}

/// 「设置 API Key…」的输入结果
pub struct KeyInput {
    pub api_key: String,
    pub model: String,
}

fn label(text: &str, frame: NSRect) -> id {
    unsafe {
        let l: id = msg_send![class!(NSTextField), alloc];
        let l: id = msg_send![l, initWithFrame: frame];
        let _: () = msg_send![l, setStringValue: nsstr(text)];
        let _: () = msg_send![l, setBezeled: NO];
        let _: () = msg_send![l, setEditable: NO];
        let _: () = msg_send![l, setSelectable: NO];
        let _: () = msg_send![l, setDrawsBackground: NO];
        l
    }
}

fn field(value: &str, placeholder: &str, frame: NSRect) -> id {
    unsafe {
        let f: id = msg_send![class!(NSTextField), alloc];
        let f: id = msg_send![f, initWithFrame: frame];
        let _: () = msg_send![f, setStringValue: nsstr(value)];
        let _: () = msg_send![f, setPlaceholderString: nsstr(placeholder)];
        let _: () = msg_send![f, setEditable: YES];
        f
    }
}

/// 配置 Key 的对话框。返回 None 表示用户取消。
///
/// 三个按钮：保存 / 取消 / 如何获取 Key…（第三个会打开 README 对应章节，
/// 并把已经输入的内容原样带回，重新弹一次，不丢用户填的东西）。
pub fn key_settings_dialog(mut key: String, mut model: String) -> Option<KeyInput> {
    if selftest_mode() {
        // 用一个固定长度的假 Key，只验证「走通保存链路」；自检跑在临时 HOME 下，
        // 不会碰用户真实的 config.json
        eprintln!("[自检] 设置对话框：自检模式返回固定的假 Key（不读输入框）");
        return Some(KeyInput {
            api_key: "selftest-ark-key-0000".to_string(),
            model: "selftest-model".to_string(),
        });
    }
    loop {
        let choice = unsafe {
            let view: id = msg_send![class!(NSView), alloc];
            let view: id = msg_send![view,
                initWithFrame: NSRect::new(NSPoint::new(0., 0.), NSSize::new(420., 96.))];

            let key_label = label("豆包 Ark API Key（必填）", NSRect::new(NSPoint::new(0., 72.), NSSize::new(420., 16.)));
            let key_field = field(&key, "粘贴你的 Ark API Key", NSRect::new(NSPoint::new(0., 46.), NSSize::new(420., 24.)));
            let model_label = label("模型 / 推理接入点 ID（可留空，用默认模型）", NSRect::new(NSPoint::new(0., 26.), NSSize::new(420., 16.)));
            let model_field = field(&model, "doubao-seed-2-0-mini-260428", NSRect::new(NSPoint::new(0., 0.), NSSize::new(420., 24.)));
            let _: () = msg_send![view, addSubview: key_label];
            let _: () = msg_send![view, addSubview: key_field];
            let _: () = msg_send![view, addSubview: model_label];
            let _: () = msg_send![view, addSubview: model_field];

            let alert: id = msg_send![class!(NSAlert), new];
            let _: () = msg_send![alert, setMessageText: nsstr("BeeK 翻译服务设置")];
            let _: () = msg_send![alert, setInformativeText: nsstr(
                "Key 只保存在本机：~/Library/Application Support/BeeK/config.json。\n\
                 也可以改用环境变量 BEEK_ARK_KEY（此处的配置优先）。"
            )];
            let _: () = msg_send![alert, setAccessoryView: view];
            let _: () = msg_send![alert, addButtonWithTitle: nsstr("保存")];
            let _: () = msg_send![alert, addButtonWithTitle: nsstr("取消")];
            let _: () = msg_send![alert, addButtonWithTitle: nsstr("如何获取 Key…")];

            // 让光标直接落在 Key 输入框里
            let win: id = msg_send![alert, window];
            let _: () = msg_send![win, makeFirstResponder: key_field];

            let resp: NSInteger = msg_send![alert, runModal];
            let idx = (resp - 1000).max(0) as usize;

            let k = nsstring_value(msg_send![key_field, stringValue]);
            let m = nsstring_value(msg_send![model_field, stringValue]);
            (idx, k, m)
        };

        let (idx, k, m) = choice;
        match idx {
            0 => {
                return Some(KeyInput {
                    api_key: k.trim().to_string(),
                    model: m.trim().to_string(),
                })
            }
            2 => {
                // 先去看怎么拿 Key，回来接着填：把已输入内容带回
                crate::update::open_url(crate::update::README_KEY_URL);
                key = k;
                model = m;
                continue;
            }
            _ => return None,
        }
    }
}

/// 「关于 BeeK」：版本信息 + 两个跳转入口。返回被点按钮下标
/// （0 = 检查更新…，1 = 打开 README，2 = 好）
pub fn about_dialog() -> usize {
    let (version, build) = crate::update::bundle_version();
    let body = format!(
        "轻量 macOS AI 划词翻译工具\n\n\
         版本 v{}（构建号 {}）\n\
         仓库 github.com/{}\n\n\
         选中文本 → 点小蜜蜂 → 看译文。\n\
         单个词会给词典式详解（读音、词性、例句、搭配），点喇叭可朗读。",
        version,
        build,
        crate::update::REPO
    );
    // 标题不再带 emoji：NSAlert 的图标本来就是 app 自己的图标（真实素材），
    // 再写一个 🐝 字形等于在真图标下面又贴了个假图标。
    alert("BeeK", &body, &["检查更新…", "打开 README", "好"])
}
