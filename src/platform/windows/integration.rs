use super::util;
use windows::{
    core::w,
    Win32::{System::Registry::*, UI::Shell::*, UI::WindowsAndMessaging::SW_SHOWNORMAL},
};
type UpdateWorker = (
    std::sync::mpsc::SyncSender<u64>,
    std::sync::Arc<std::sync::Mutex<Option<(u64, String)>>>,
    std::sync::Arc<std::sync::atomic::AtomicU64>,
);
static UPDATES: std::sync::OnceLock<UpdateWorker> = std::sync::OnceLock::new();
pub fn open_updates() -> bool {
    let (tx, result, current) = UPDATES.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let result = std::sync::Arc::new(std::sync::Mutex::new(None));
        let latest = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let out = result.clone();
        let current = latest.clone();
        std::thread::spawn(move || {
            while let Ok(id) = rx.recv() {
                let (status, open) = check_update();
                let accepted = {
                    let mut slot = out.lock().unwrap();
                    if current.load(std::sync::atomic::Ordering::Acquire) == id {
                        *slot = Some((id, status));
                        true
                    } else {
                        false
                    }
                };
                if accepted && open {
                    unsafe {
                        let _ = ShellExecuteW(
                            None,
                            w!("open"),
                            w!("https://github.com/kaisersong/beek/releases"),
                            None,
                            None,
                            SW_SHOWNORMAL,
                        );
                    }
                }
            }
        });
        (tx, result, latest)
    });
    // UI calls are serialized. Publish the generation before waking the worker.
    let mut slot = result.lock().unwrap();
    let previous = current.load(std::sync::atomic::Ordering::Acquire);
    let id = previous.wrapping_add(1);
    current.store(id, std::sync::atomic::Ordering::Release);
    if tx.try_send(id).is_err() {
        current.store(previous, std::sync::atomic::Ordering::Release);
        return false;
    }
    *slot = None;
    true
}
fn check_update() -> (String, bool) {
    use std::{io::Read, time::Duration};
    let result = (|| -> Result<Option<String>, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "无法初始化更新检查")?;
        let mut response = client
            .get("https://api.github.com/repos/kaisersong/beek/releases/latest")
            .header("User-Agent", concat!("BeeK/", env!("CARGO_PKG_VERSION")))
            .send()
            .map_err(|_| "更新检查失败，请稍后重试")?;
        if !response.status().is_success() {
            return Err(format!(
                "更新检查失败（HTTP {}）",
                response.status().as_u16()
            ));
        }
        let mut bytes = Vec::new();
        response
            .by_ref()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "更新响应读取失败")?;
        if bytes.len() > 1024 * 1024 {
            return Err("更新响应超出限制".into());
        }
        let release = serde_json::from_slice(&bytes).map_err(|_| "更新信息格式无效")?;
        crate::core::release::windows_update(&release, env!("CARGO_PKG_VERSION"))
    })();
    match result {
        Ok(None) => ("当前已是最新 Windows 版本".into(), false),
        Ok(Some(version)) => (
            format!("发现 {version}，已打开固定发布页；请下载并检查签名后安装"),
            true,
        ),
        Err(error) => (error, false),
    }
}
pub fn update_result() -> Option<String> {
    let (_, result, current) = UPDATES.get()?;
    let (id, status) = result.lock().ok()?.take()?;
    if current.load(std::sync::atomic::Ordering::Acquire) == id {
        Some(status)
    } else {
        None
    }
}
pub fn autostart(enabled: bool) -> Result<(), String> {
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()
        .map_err(|_| "配置已保存，但无法修改登录启动项")?;
        let result = if enabled {
            let path = std::env::current_exe().map_err(|_| "程序路径不可用")?;
            let value = util::wide(format!("\"{}\" --autostart", path.display()));
            let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
            RegSetValueExW(key, w!("BeeK"), None, REG_SZ, Some(bytes)).ok()
        } else {
            let r = RegDeleteValueW(key, w!("BeeK"));
            if r == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                r.ok()
            }
        };
        let _ = RegCloseKey(key);
        result.map_err(|_| "配置已保存，但登录启动项修改失败".into())
    }
}
