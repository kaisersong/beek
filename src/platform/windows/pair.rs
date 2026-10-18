use windows::{core::*, Win32::Storage::FileSystem::*};
pub fn helper() -> std::result::Result<std::path::PathBuf, String> {
    let helper = std::env::current_exe()
        .map_err(|_| "无法定位 BeeK 程序")?
        .with_file_name("beek-selection-host.exe");
    let path = super::util::wide(&helper);
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(path.as_ptr()), None);
        if size == 0 || size > 1024 * 1024 {
            return Err("选区助手缺失或版本信息无效，请重新安装完整 BeeK".into());
        }
        let mut bytes = vec![0u8; size as usize];
        GetFileVersionInfoW(
            PCWSTR(path.as_ptr()),
            None,
            size,
            bytes.as_mut_ptr() as *mut _,
        )
        .map_err(|_| "无法读取选区助手版本")?;
        let mut ptr = std::ptr::null_mut();
        let mut length = 0;
        if !VerQueryValueW(
            bytes.as_ptr() as *const _,
            w!("\\StringFileInfo\\040904b0\\BeeKBuildId"),
            &mut ptr,
            &mut length,
        )
        .as_bool()
            || ptr.is_null()
            || length != 65
        {
            return Err("选区助手与主程序不配套，请重新安装完整 BeeK".into());
        }
        let value = String::from_utf16(std::slice::from_raw_parts(ptr as *const u16, 64))
            .map_err(|_| "助手版本信息编码无效")?;
        if value != env!("BEEK_BUILD_ID") {
            return Err("选区助手与主程序构建不一致，请重新安装完整 BeeK".into());
        }
    }
    Ok(helper)
}
