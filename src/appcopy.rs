// 通过「目标应用自己」取选中文本 —— 参考 SelectedTextKit（Easydict 的取词组件）
//
// 两条路，都是让应用自己把选区交出来，而不是我们硬塞合成按键：
//   1. 浏览器：AppleScript 执行 window.getSelection()（确定性，不碰剪贴板）
//   2. 通用：按目标应用的「拷贝」菜单项（AXPress），前后备份/还原剪贴板
// 合成的 ⌘C 只作为最后手段（实测 Electron/Safari 常常收不到）。
use crate::clipboard;

#[derive(Debug)]
pub enum Outcome {
    /// 拿到了文本
    Copied(String),
    /// 应用明确表示「没有可拷贝的内容」→ 用户没选中
    NoSelection,
    /// 这条路走不通（没有相应能力）
    Unavailable,
}

/// 常见浏览器的 (bundle id, 应用名) 映射 —— AppleScript 用得上
const BROWSERS: &[(&str, &str)] = &[
    ("com.apple.Safari", "Safari"),
    ("com.apple.SafariTechnologyPreview", "Safari Technology Preview"),
    ("com.google.Chrome", "Google Chrome"),
    ("com.google.Chrome.canary", "Google Chrome Canary"),
    ("com.microsoft.edgemac", "Microsoft Edge"),
    ("com.brave.Browser", "Brave Browser"),
    ("company.thebrowser.Browser", "Arc"),
    ("org.chromium.Chromium", "Chromium"),
    ("com.vivaldi.Vivaldi", "Vivaldi"),
    ("com.operasoftware.Opera", "Opera"),
];

fn browser_name(bundle_id: &str) -> Option<&'static str> {
    BROWSERS
        .iter()
        .find(|(id, _)| *id == bundle_id)
        .map(|(_, name)| *name)
}

/// 带超时地跑一个子进程（超时就 kill，返回 None）
fn run_with_timeout(
    cmd: &mut std::process::Command,
    timeout: std::time::Duration,
) -> Option<std::process::Output> {
    use std::process::Stdio;
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    eprintln!(
                        "[beek] AppleScript 取词超时（{}s）→ 放弃这条路，改走菜单拷贝",
                        timeout.as_secs()
                    );
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// 「自动化被拒绝」只提示一次（用户拒绝了就会一直失败，没必要每次划词都刷屏）
fn log_refused_once(err: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static SHOWN: AtomicBool = AtomicBool::new(false);
    if SHOWN.swap(true, Ordering::Relaxed) {
        return;
    }
    eprintln!(
        "[beek] AppleScript 取词被拒绝（{err}）。\
         系统会记住这次「不允许」、不再弹窗；要恢复这条路径请到：\
         系统设置 → 隐私与安全性 → 自动化 → BeeK → 勾上对应的浏览器。\
         不恢复也能用：取词会继续走「按应用自己的拷贝菜单」和剪贴板兜底。"
    );
}

/// 用 AppleScript 问浏览器要选区。Safari 与 Chromium 系的脚本不同：
/// Chromium 要 execute active tab 的 javascript，且脚本字符串需要再嵌一层引号。
fn applescript_selection(app_name: &str, is_safari: bool) -> Option<String> {
    let js = "window.getSelection().toString()";
    let script = if is_safari {
        format!(
            "tell application \"{}\" to do JavaScript \"{}\" in document 1",
            app_name, js
        )
    } else {
        format!(
            "tell application \"{}\" to execute front window's active tab javascript \"{}\"",
            app_name, js
        )
    };
    // 必须带超时：osascript 无超时会一直等目标应用回话，而这段代码跑在 AX 轮询线程里，
    // 浏览器一卡住整个取词就停摆（返回 None 比拖死强）。
    let out = run_with_timeout(
        std::process::Command::new("/usr/bin/osascript").arg("-e").arg(&script),
        std::time::Duration::from_secs(2),
    )?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        // -1743 / errAEEventNotPermitted：用户第一次弹窗点了「不允许」。
        // 系统会记住这个决定、不再弹窗，只能去 系统设置 → 隐私与安全性 → 自动化 里手动勾。
        // 取词不会因此坏掉（后面还有菜单拷贝那条），但建议把原因写清楚、别只丢一行 stderr。
        if err.contains("1743") || err.contains("Not authorized") {
            log_refused_once(&err);
        } else {
            eprintln!("[beek] AppleScript 取词失败：{}", err);
        }
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Some(text)
}

/// 让目标应用自己把选中文本交出来。
/// `pid` = 前台应用；`bundle_id` = 它的 bundle id（拿不到就传空）
pub fn selected_text_via_app(pid: i32, bundle_id: &str) -> Outcome {
    // 1) 浏览器：AppleScript（不碰剪贴板，最干净）
    if let Some(name) = browser_name(bundle_id) {
        let is_safari = bundle_id.starts_with("com.apple.Safari");
        match applescript_selection(name, is_safari) {
            Some(t) if !t.trim().is_empty() => {
                eprintln!("[beek] 取词：AppleScript 从 {} 拿到 {:?}", name, t.chars().take(20).collect::<String>());
                return Outcome::Copied(t);
            }
            Some(_) => {
                // 空字符串：可能是没选中，也可能是该浏览器不允许 JS（比如「允许 Apple 事件中的 JavaScript」未开）
                eprintln!("[beek] 取词：AppleScript 从 {} 拿到空字符串，改用菜单拷贝", name);
            }
            None => {}
        }
    }

    // 2) 通用：按目标应用的「拷贝」菜单项
    match unsafe { crate::ax::menu_copy_item(pid) } {
        Some((_item, false)) => {
            eprintln!("[beek] 取词：目标应用的「拷贝」菜单项是灰的 → 判定为没有选中");
            Outcome::NoSelection
        }
        Some((item, true)) => {
            let snap = clipboard::snapshot();
            let before = clipboard::change_count();
            let pressed = unsafe { crate::ax::press(item) };
            if !pressed {
                eprintln!("[beek] 取词：AXPress 菜单项失败");
                return Outcome::Unavailable;
            }
            // 等剪贴板变化（应用的拷贝是异步的）
            let mut waited = 0;
            while waited < 600 {
                std::thread::sleep(std::time::Duration::from_millis(40));
                waited += 40;
                if clipboard::change_count() != before {
                    break;
                }
            }
            let changed = clipboard::change_count() != before;
            let text = if changed { clipboard::read_string() } else { None };
            // 无论拿到没有，都把剪贴板还原（菜单拷贝也会覆盖剪贴板）
            match snap {
                Some(s) => clipboard::restore(&s),
                None => clipboard::clear(),
            }
            match text {
                Some(t) if !t.trim().is_empty() => {
                    eprintln!(
                        "[beek] 取词：菜单拷贝拿到 {:?}",
                        t.chars().take(20).collect::<String>()
                    );
                    Outcome::Copied(t)
                }
                _ => {
                    eprintln!("[beek] 取词：按了菜单拷贝但剪贴板没变化");
                    Outcome::Unavailable
                }
            }
        }
        None => {
            eprintln!("[beek] 取词：该应用菜单里找不到「拷贝」项");
            Outcome::Unavailable
        }
    }
}

/// 前台应用 pid（复用剪贴板模块里的实现）
pub fn frontmost_pid() -> Option<i32> {
    clipboard::frontmost_pid()
}

/// 取前台应用的 bundle id（AppleScript 分支用）
pub fn frontmost_bundle_id() -> String {
    unsafe {
        use cocoa::base::{id, nil};
        use objc::{class, msg_send, sel, sel_impl};
        let ws: id = msg_send![class!(NSWorkspace), sharedWorkspace];
        let app: id = msg_send![ws, frontmostApplication];
        if app == nil {
            return String::new();
        }
        let bid: id = msg_send![app, bundleIdentifier];
        if bid == nil {
            return String::new();
        }
        let ptr: *const std::os::raw::c_char = msg_send![bid, UTF8String];
        if ptr.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}
