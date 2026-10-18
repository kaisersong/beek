use crate::core::protocol::{Rect, Source};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        Security::*,
        System::{
            Com::CoTaskMemFree, RemoteDesktop::ProcessIdToSessionId, StationsAndDesktops::*,
            Threading::*,
        },
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

pub fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    value.as_ref().encode_wide().chain(Some(0)).collect()
}
pub fn nonce() -> std::result::Result<String, String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|_| "系统随机数不可用")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub struct Handle(pub HANDLE);
// Kernel handles (unlike HWND/COM pointers) may be closed/waited from any thread.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}
pub fn hwnd(value: u64) -> HWND {
    HWND(value as usize as *mut _)
}
pub fn source(window: HWND, generation: u64) -> Option<Source> {
    unsafe {
        if !IsWindow(Some(window)).as_bool() {
            return None;
        }
        let window = GetAncestor(window, GA_ROOT);
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let p = Handle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
        let (mut created, mut exit, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        GetProcessTimes(p.0, &mut created, &mut exit, &mut kernel, &mut user).ok()?;
        let created = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
        if !eligible(p.0, pid, created) {
            return None;
        }
        Some(Source {
            hwnd: window.0 as u64,
            pid,
            created,
            generation,
        })
    }
}
struct Token {
    sid: Vec<u8>,
    elevated: bool,
    integrity: u32,
}
fn token(process: HANDLE) -> Option<Token> {
    unsafe {
        let mut h = HANDLE::default();
        OpenProcessToken(process, TOKEN_QUERY, &mut h).ok()?;
        let h = Handle(h);
        let mut size = 0;
        let _ = GetTokenInformation(h.0, TokenUser, None, 0, &mut size);
        if size == 0 || size > 64 * 1024 {
            return None;
        }
        let mut bytes = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(
            h.0,
            TokenUser,
            Some(bytes.as_mut_ptr() as *mut _),
            size,
            &mut size,
        )
        .ok()?;
        let sid = (*(bytes.as_ptr() as *const TOKEN_USER)).User.Sid;
        let n = GetLengthSid(sid);
        if n == 0 || n > 256 {
            return None;
        }
        let sid = std::slice::from_raw_parts(sid.0 as *const u8, n as usize).to_vec();
        let mut elevation = TOKEN_ELEVATION::default();
        GetTokenInformation(
            h.0,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of_val(&elevation) as u32,
            &mut size,
        )
        .ok()?;
        let mut needed = 0;
        let _ = GetTokenInformation(h.0, TokenIntegrityLevel, None, 0, &mut needed);
        if needed == 0 || needed > 65536 {
            return None;
        }
        let mut label = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(
            h.0,
            TokenIntegrityLevel,
            Some(label.as_mut_ptr() as *mut _),
            needed,
            &mut needed,
        )
        .ok()?;
        let integrity_sid = (*(label.as_ptr() as *const TOKEN_MANDATORY_LABEL))
            .Label
            .Sid;
        let count = *GetSidSubAuthorityCount(integrity_sid);
        if count == 0 {
            return None;
        }
        let integrity = *GetSidSubAuthority(integrity_sid, (count - 1) as u32);
        Some(Token {
            sid,
            elevated: elevation.TokenIsElevated != 0,
            integrity,
        })
    }
}
fn eligible(process: HANDLE, pid: u32, created: u64) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<(u32, u64), bool>>> = OnceLock::new();
    static OWN: OnceLock<Option<Token>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    if let Some(value) = cache.get(&(pid, created)) {
        return *value;
    }
    let own = OWN.get_or_init(|| token(unsafe { GetCurrentProcess() }));
    let mut session = 0;
    let mut own_session = 0;
    let valid = unsafe {
        ProcessIdToSessionId(pid, &mut session).is_ok()
            && ProcessIdToSessionId(GetCurrentProcessId(), &mut own_session).is_ok()
    } && session == own_session
        && own
            .as_ref()
            .zip(token(process))
            .is_some_and(|(own, other)| {
                own.sid == other.sid
                    && other.integrity <= own.integrity
                    && (!other.elevated || own.elevated)
            });
    if cache.len() >= 64 {
        cache.clear();
    }
    cache.insert((pid, created), valid);
    valid
}
pub fn current(generation: u64) -> Option<Source> {
    if super::native::MAIN_STARTED.load(std::sync::atomic::Ordering::Acquire) {
        if !super::native::DESKTOP_ACTIVE.load(std::sync::atomic::Ordering::Acquire) {
            return None;
        }
    } else if !desktop_available() {
        return None;
    }
    source(unsafe { GetForegroundWindow() }, generation)
        .filter(|s| s.pid != unsafe { GetCurrentProcessId() })
}
pub fn desktop_available() -> bool {
    unsafe {
        let Ok(input) = OpenInputDesktop(
            DESKTOP_CONTROL_FLAGS(0),
            false,
            DESKTOP_ACCESS_FLAGS(DESKTOP_SWITCHDESKTOP.0 | DESKTOP_READOBJECTS.0),
        ) else {
            return false;
        };
        struct Desktop(HDESK);
        impl Drop for Desktop {
            fn drop(&mut self) {
                unsafe {
                    let _ = CloseDesktop(self.0);
                }
            }
        }
        let _guard = Desktop(input);
        let Ok(thread) = GetThreadDesktop(GetCurrentThreadId()) else {
            return false;
        };
        fn name(h: HDESK) -> Option<Vec<u16>> {
            let mut value = [0u16; 256];
            let mut n = 0;
            unsafe {
                GetUserObjectInformationW(
                    HANDLE(h.0),
                    UOI_NAME,
                    Some(value.as_mut_ptr() as *mut _),
                    512,
                    Some(&mut n),
                )
                .ok()?;
            }
            let end = value.iter().position(|n| *n == 0)?;
            Some(value[..end].to_vec())
        }
        name(input).zip(name(thread)).is_some_and(|(a, b)| a == b)
    }
}
pub fn is_current(s: Source) -> bool {
    current(s.generation) == Some(s)
}
pub fn identity_alive(s: Source) -> bool {
    source(hwnd(s.hwnd), s.generation) == Some(s)
}
pub fn process_alive(pid: u32, created: u64) -> bool {
    unsafe {
        let Ok(h) = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            pid,
        ) else {
            return false;
        };
        let h = Handle(h);
        if WaitForSingleObject(h.0, 0) != WAIT_TIMEOUT {
            return false;
        }
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        GetProcessTimes(h.0, &mut c, &mut e, &mut k, &mut u).is_ok()
            && (((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64) == created
    }
}
pub fn known_folder(local: bool) -> std::result::Result<PathBuf, String> {
    unsafe {
        let p = SHGetKnownFolderPath(
            if local {
                &FOLDERID_LocalAppData
            } else {
                &FOLDERID_RoamingAppData
            },
            KF_FLAG_DEFAULT,
            None,
        )
        .map_err(|_| "无法定位 Windows 用户数据目录")?;
        let result = p
            .to_string()
            .map(PathBuf::from)
            .map_err(|_| "用户数据目录包含无效字符".to_owned());
        CoTaskMemFree(Some(p.0 as *const _));
        result.map(|p| p.join("BeeK"))
    }
}
pub fn cursor() -> [i32; 2] {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    [p.x, p.y]
}
pub fn work_area(point: [i32; 2]) -> Rect {
    unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: point[0],
                y: point[1],
            },
            MONITOR_DEFAULTTONEAREST,
        );
        let mut m = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut m).as_bool() {
            return Rect {
                x: 0.,
                y: 0.,
                width: 800.,
                height: 600.,
            };
        }
        Rect {
            x: m.rcWork.left as f64,
            y: m.rcWork.top as f64,
            width: (m.rcWork.right - m.rcWork.left) as f64,
            height: (m.rcWork.bottom - m.rcWork.top) as f64,
        }
    }
}
pub fn fatal(message: &str) {
    let m = wide(message);
    unsafe {
        MessageBoxW(None, PCWSTR(m.as_ptr()), w!("BeeK"), MB_OK | MB_ICONERROR);
    }
}
pub fn native_machine() -> Option<u16> {
    let mut process = windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE::default();
    let mut native = process;
    unsafe {
        IsWow64Process2(GetCurrentProcess(), &mut process, Some(&mut native)).ok()?;
    }
    Some(native.0)
}
pub fn check_platform() -> std::result::Result<(), String> {
    if native_machine()
        != Some(windows::Win32::System::SystemInformation::IMAGE_FILE_MACHINE_AMD64.0)
    {
        return Err("首版仅支持原生 Windows 11 x64，不支持 ARM64 仿真".into());
    }
    if !cfg!(target_arch = "x86_64") {
        return Err("首版 BeeK 需要 Windows 11 x64".into());
    }
    if token(unsafe { GetCurrentProcess() }).is_none_or(|t| t.elevated) {
        return Err("请以普通用户权限启动 BeeK，无需管理员权限".into());
    }
    use windows::Win32::System::SystemInformation::OSVERSIONINFOEXW;
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(info: *mut OSVERSIONINFOEXW) -> i32;
    }
    let mut version = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOEXW>() as u32,
        ..Default::default()
    };
    if unsafe { RtlGetVersion(&mut version) } < 0
        || version.dwMajorVersion < 10
        || version.dwBuildNumber < 22000
        || version.wProductType != 1
    {
        return Err("BeeK 首版需要 Windows 11 x64 桌面系统".into());
    }
    Ok(())
}
