//! G0 native adapter. Every HWND comes from raw-window-handle or our own window.
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    sync::{OnceLock, mpsc},
    thread,
};
use windows::{
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::*,
        UI::{HiDpi::*, Shell::*, WindowsAndMessaging::*},
    },
    core::*,
};

static EVENTS: OnceLock<mpsc::SyncSender<super::Command>> = OnceLock::new();
const TRAY_MESSAGE: u32 = WM_APP + 1;
const RESTORE: usize = 100;
const OVERLAY: usize = 101;
const QUIT: usize = 102;
const HIDE_ALL: usize = 103;

pub fn foreground() -> usize {
    unsafe { GetForegroundWindow().0 as usize }
}

pub fn record_lifetime_host(window: &gpui::Window) {
    let h = hwnd(window).expect("lifecycle host must expose its HWND");
    super::record(
        "lifetime_host",
        serde_json::json!({
            "hwnd": h.0 as usize,
            "visible": unsafe { IsWindowVisible(h).as_bool() }
        }),
    );
}

fn hwnd(window: &gpui::Window) -> Result<HWND> {
    let handle = HasWindowHandle::window_handle(window)
        .map_err(|_| Error::from_hresult(HRESULT(0x80004005u32 as i32)))?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Ok(HWND(h.hwnd.get() as *mut _)),
        _ => Err(Error::from_hresult(HRESULT(0x80004005u32 as i32))),
    }
}

unsafe extern "system" fn no_activate(
    window: HWND,
    message: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    if message == WM_MOUSEACTIVATE {
        return LRESULT(MA_NOACTIVATE as isize);
    }
    if message == WM_NCDESTROY {
        unsafe {
            let _ = RemoveWindowSubclass(window, Some(no_activate), id);
        }
    }
    unsafe { DefSubclassProc(window, message, w, l) }
}

/// Called while GPUI's popup is still hidden, before its first display.
pub fn show_overlay(window: &gpui::Window) -> Result<()> {
    let h = hwnd(window)?;
    unsafe {
        let styles = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        SetLastError(WIN32_ERROR(0));
        let old = SetWindowLongPtrW(
            h,
            GWL_EXSTYLE,
            (styles | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) as isize,
        );
        if old == 0 && GetLastError().0 != 0 {
            return Err(Error::from_win32());
        }
        if !SetWindowSubclass(h, Some(no_activate), 1, 0).as_bool() {
            return Err(Error::from_win32());
        }
        let mut cursor = POINT::default();
        GetCursorPos(&mut cursor)?;
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        GetMonitorInfoW(monitor, &mut info).ok()?;
        // Place on the target monitor first; WM_DPICHANGED lets GPUI update scale.
        SetWindowPos(
            h,
            Some(HWND_TOPMOST),
            info.rcWork.left + 24,
            info.rcWork.top + 48,
            220,
            76,
            SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )?;
        let scale = GetDpiForWindow(h).max(96) as f32 / 96.;
        SetWindowPos(
            h,
            Some(HWND_TOPMOST),
            info.rcWork.left + 24,
            info.rcWork.top + 48,
            (220. * scale).round() as i32,
            (76. * scale).round() as i32,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )?;
        super::record(
            "overlay_native",
            serde_json::json!({
                "hwnd": h.0 as usize, "dpi": GetDpiForWindow(h),
                "x": info.rcWork.left + 24, "y": info.rcWork.top + 48,
                "extended_style": GetWindowLongPtrW(h, GWL_EXSTYLE),
                "foreground": foreground()
            }),
        );
    }
    Ok(())
}

fn notify(window: HWND, action: NOTIFY_ICON_MESSAGE) -> Result<()> {
    unsafe {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: window,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY_MESSAGE,
            hIcon: LoadIconW(None, IDI_APPLICATION)?,
            ..Default::default()
        };
        let tip: Vec<u16> = "BeeK G0 probe\0".encode_utf16().collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);
        if Shell_NotifyIconW(action, &data).as_bool() {
            Ok(())
        } else {
            Err(Error::from_win32())
        }
    }
}

fn dispatch(command: usize) {
    let event = match command {
        RESTORE => super::Command::Input,
        OVERLAY => super::Command::Overlay,
        QUIT => super::Command::Quit,
        HIDE_ALL => super::Command::HideAll,
        _ => return,
    };
    if let Some(tx) = EVENTS.get() {
        let _ = tx.try_send(event);
    }
}

unsafe extern "system" fn procedure(h: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if message == unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) } {
        if let Err(e) = notify(h, NIM_ADD) {
            super::record("tray_error", serde_json::json!({"code": e.code().0}));
        } else {
            super::record("tray_recreated", serde_json::json!({}));
        }
        return LRESULT(0);
    }
    match message {
        TRAY_MESSAGE if l.0 as u32 == WM_LBUTTONUP => dispatch(RESTORE),
        TRAY_MESSAGE if l.0 as u32 == WM_RBUTTONUP => unsafe {
            if let Ok(menu) = CreatePopupMenu() {
                let _ = AppendMenuW(menu, MF_STRING, RESTORE, w!("Open input / IME probe"));
                let _ = AppendMenuW(menu, MF_STRING, OVERLAY, w!("Show non-activating popup"));
                let _ = AppendMenuW(menu, MF_STRING, HIDE_ALL, w!("Close all probe windows"));
                let _ = AppendMenuW(menu, MF_STRING, QUIT, w!("Exit probe"));
                let mut p = POINT::default();
                let _ = GetCursorPos(&mut p);
                let _ = SetForegroundWindow(h);
                let selected =
                    TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY, p.x, p.y, None, h, None);
                dispatch(selected.0 as usize);
                let _ = PostMessageW(Some(h), WM_NULL, WPARAM(0), LPARAM(0));
                let _ = DestroyMenu(menu);
            }
        },
        WM_CLOSE => unsafe {
            let _ = DestroyWindow(h);
        },
        WM_DESTROY => unsafe {
            let _ = notify(h, NIM_DELETE);
            PostQuitMessage(0);
        },
        _ => return unsafe { DefWindowProcW(h, message, w, l) },
    }
    LRESULT(0)
}

/// The owner HWND is a hidden top-level window (not HWND_MESSAGE): it must
/// receive Explorer's TaskbarCreated broadcast after Explorer restarts.
pub struct Tray {
    window: usize,
    thread: Option<thread::JoinHandle<()>>,
}
impl Tray {
    pub fn start(events: mpsc::SyncSender<super::Command>) -> std::result::Result<Self, String> {
        EVENTS
            .set(events)
            .map_err(|_| "probe tray already running".to_owned())?;
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || unsafe {
            let init = (|| -> Result<HWND> {
                let instance = GetModuleHandleW(None)?;
                let class = WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: instance.into(),
                    lpszClassName: w!("BeeKG0Tray"),
                    ..Default::default()
                };
                if RegisterClassW(&class) == 0 {
                    return Err(Error::from_win32());
                }
                let h = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("BeeKG0Tray"),
                    w!("BeeK G0 tray owner"),
                    WINDOW_STYLE(0),
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                )?;
                if let Err(e) = notify(h, NIM_ADD) {
                    let _ = DestroyWindow(h);
                    return Err(e);
                }
                Ok(h)
            })();
            match init {
                Ok(h) => {
                    if ready_tx.send(Ok(h.0 as usize)).is_err() {
                        let _ = DestroyWindow(h);
                        return;
                    }
                    super::record("tray_ready", serde_json::json!({}));
                    let mut msg = MSG::default();
                    loop {
                        let result = GetMessageW(&mut msg, None, 0, 0).0;
                        if result <= 0 {
                            break;
                        }
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    let _ = notify(h, NIM_DELETE);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("tray initialization: {}", e.code().0)));
                }
            }
        });
        let window = ready_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| "tray initialization timed out".to_owned())??;
        Ok(Self {
            window,
            thread: Some(worker),
        })
    }
}
impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.window as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        // Allow the owner thread to delete the notification icon before process
        // exit, with a strict bound even if Windows' menu thread is unresponsive.
        let started = std::time::Instant::now();
        while self.thread.as_ref().is_some_and(|t| !t.is_finished())
            && started.elapsed() < std::time::Duration::from_millis(500)
        {
            thread::sleep(std::time::Duration::from_millis(5));
        }
        if self.thread.as_ref().is_some_and(|t| t.is_finished()) {
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}
