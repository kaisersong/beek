# 收尾增量复审 R4

只审阅下面新增实现和调用边界，不扩展此前已关闭的问题。四个子系统已有限PASS；G5依旧未通过，不得据此批准发布。

1. 实测实时卡底部按钮未显示：GetClientRect对应约520x420，但GPUI viewport仍1424x744。SetWindowPos在GPUI Window update内部同步触发resize回调，Window重复借用失败。现在仅对自己的popup，PostMessage一个无指针私有消息，在消息循环重新读取当前client尺寸并转交原窗口过程WM_SIZE，不激活、不修改焦点、不改GPUI源码。请核查生命周期、DPI、重入以及消息处理边界。
2. 固定STA语音工作线程，1活动+1队列、2个有界结果；延迟至首次朗读才初始化COM/SAPI，避免UI消息循环开始前的COM激活。错误给用户反馈，结果按generation丢弃过时回执，不join阻塞语音线程。
3. 实时卡现在持续核对source仍为当前前台，避免WINEVENT_SKIPOWNPROCESS漏掉Settings激活；快照不受影响。
4. Settings/快照打开前钳制工作区。启动增加结构性phase日志（不含配置/正文）。
5. 之前跨子系统留观：restart失败事件明确restart_events=None，正常交接只在Committed时commit_exit；失败子不弹独立fatal。完整调用点附下。

请独立寻找真实可触发缺陷，缺依赖时明确标假设，不把假设定性为缺陷。最终给PASS或REVISE，逐项列出仍需修复内容。源码有限PASS与环境验收区分。

## src/platform/windows/native.rs
```rust
use super::util;
use crate::core::protocol::Rect;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{
            DataExchange::*, LibraryLoader::*, RemoteDesktop::*, Threading::GetCurrentProcessId,
        },
        UI::{
            Accessibility::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*,
            WindowsAndMessaging::*,
        },
    },
};

#[derive(Clone, Copy)]
pub enum Event {
    Settings,
    Clipboard,
    Hotkey,
    Restore,
    Restart,
    About,
    Quit,
    ClipboardChanged,
    IpcUnavailable,
}
static EVENTS: OnceLock<mpsc::SyncSender<Event>> = OnceLock::new();
pub static EPOCH: AtomicU64 = AtomicU64::new(1);
pub static SYSTEM_EPOCH: AtomicU64 = AtomicU64::new(1);
pub static MAIN_STARTED: AtomicBool = AtomicBool::new(false);
pub static DESKTOP_ACTIVE: AtomicBool = AtomicBool::new(true);
pub static SESSION_NOTIFICATIONS: AtomicBool = AtomicBool::new(false);
static LOCKED: AtomicBool = AtomicBool::new(false);
static TRIGGER: AtomicU64 = AtomicU64::new(0);
static CLOCK: OnceLock<Instant> = OnceLock::new();
static DRAG: AtomicBool = AtomicBool::new(false);
static MOUSE_INPUT: AtomicBool = AtomicBool::new(false);
static MOUSE_AT: AtomicU64 = AtomicU64::new(0);
static MOUSE_ROOT: AtomicU64 = AtomicU64::new(0);
static MOUSE_PID: AtomicU32 = AtomicU32::new(0);
static MOUSE_POINT: AtomicU64 = AtomicU64::new(0);
pub static CLIPBOARD_HELPER_PID: AtomicU32 = AtomicU32::new(0);
static CLIPBOARD_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static CLIPBOARD_ENABLED: AtomicBool = AtomicBool::new(false);
pub static HOTKEY_OK: AtomicBool = AtomicBool::new(true);
static HOTKEY: OnceLock<Mutex<(bool, String)>> = OnceLock::new();
const TRAY: u32 = WM_APP + 1;
const CONFIG: u32 = WM_APP + 2;
fn send(e: Event) {
    if let Some(tx) = EVENTS.get() {
        let _ = tx.try_send(e);
    }
}
fn invalidate(trigger: bool) {
    EPOCH.fetch_add(1, Ordering::AcqRel);
    if trigger {
        TRIGGER.store(
            CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1,
            Ordering::Release,
        );
    }
}
pub fn selection_changed() {
    invalidate(true);
}
pub fn triggered() -> u64 {
    TRIGGER.load(Ordering::Acquire)
}
pub fn quiet_since(trigger: u64) -> bool {
    trigger > 0 && CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1 >= trigger + 120
}
pub fn mouse_anchor(source: crate::core::protocol::Source) -> Option<[i32; 2]> {
    let at = MOUSE_AT.load(Ordering::Acquire);
    let now = CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1;
    if !MOUSE_INPUT.load(Ordering::Acquire)
        || now.saturating_sub(at) > 500
        || MOUSE_ROOT.load(Ordering::Acquire) != source.hwnd
        || MOUSE_PID.load(Ordering::Acquire) != source.pid
    {
        return None;
    }
    let point = MOUSE_POINT.load(Ordering::Acquire);
    Some([(point >> 32) as u32 as i32, point as u32 as i32])
}
fn external(h: HWND) -> bool {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(h, Some(&mut pid));
        pid != 0 && pid != GetCurrentProcessId()
    }
}
unsafe extern "system" fn mouse(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if code >= 0 {
            let data = &*(l.0 as *const MSLLHOOKSTRUCT);
            let h = WindowFromPoint(data.pt);
            if external(h) {
                match w.0 as u32 {
                    WM_LBUTTONDOWN => {
                        DRAG.store(true, Ordering::Release);
                        invalidate(false);
                    }
                    WM_LBUTTONUP => {
                        DRAG.store(false, Ordering::Release);
                        let root = GetAncestor(h, GA_ROOT);
                        let mut pid = 0;
                        GetWindowThreadProcessId(root, Some(&mut pid));
                        MOUSE_ROOT.store(root.0 as u64, Ordering::Release);
                        MOUSE_PID.store(pid, Ordering::Release);
                        MOUSE_POINT.store(
                            ((data.pt.x as u32 as u64) << 32) | data.pt.y as u32 as u64,
                            Ordering::Release,
                        );
                        MOUSE_AT.store(
                            CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1,
                            Ordering::Release,
                        );
                        MOUSE_INPUT.store(true, Ordering::Release);
                        invalidate(true);
                    }
                    WM_MOUSEMOVE if DRAG.load(Ordering::Acquire) => invalidate(false),
                    WM_MOUSEWHEEL | WM_MOUSEHWHEEL => invalidate(false),
                    _ => {}
                }
            }
        }
        CallNextHookEx(None, code, w, l)
    }
}
unsafe extern "system" fn keyboard(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if code >= 0 && external(GetForegroundWindow()) {
            let data = &*(l.0 as *const KBDLLHOOKSTRUCT);
            if ![
                0x10, 0x11, 0x12, 0x5b, 0x5c, 0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5,
            ]
            .contains(&data.vkCode)
            {
                MOUSE_INPUT.store(false, Ordering::Release);
                let system_chord = data.flags.0 & LLKHF_ALTDOWN.0 != 0
                    || GetAsyncKeyState(VK_LWIN.0 as i32) < 0
                    || GetAsyncKeyState(VK_RWIN.0 as i32) < 0;
                if w.0 as u32 == WM_KEYDOWN || w.0 as u32 == WM_SYSKEYDOWN {
                    invalidate(false);
                } else {
                    invalidate(!system_chord);
                }
            }
        }
        CallNextHookEx(None, code, w, l)
    }
}
unsafe extern "system" fn changed(
    _: HWINEVENTHOOK,
    event: u32,
    h: HWND,
    object: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    // No UIA/COM calls, text, networking or synchronous IPC in callbacks.
    if event == EVENT_OBJECT_LOCATIONCHANGE
        && (unsafe { GetAncestor(h, GA_ROOT) } != unsafe { GetForegroundWindow() }
            || !matches!(object, 0 | -8))
    {
        return;
    }
    invalidate(event == EVENT_OBJECT_TEXTSELECTIONCHANGED);
}
pub fn hwnd(window: &gpui::Window) -> Result<HWND> {
    match HasWindowHandle::window_handle(window)
        .map_err(|_| Error::from_hresult(HRESULT(0x80004005u32 as i32)))?
        .as_raw()
    {
        RawWindowHandle::Win32(h) => Ok(HWND(h.hwnd.get() as *mut _)),
        _ => Err(Error::from_hresult(HRESULT(0x80004005u32 as i32))),
    }
}
unsafe extern "system" fn no_activate(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        // SetWindowPos called while GPUI owns a Window may synchronously invoke
        // on_resize before that Window can be borrowed again. Re-deliver the
        // current client size on the native event loop, after the borrow ends.
        if m == WM_APP + 23 {
            let mut rect = RECT::default();
            if GetClientRect(h, &mut rect).is_ok() {
                let width = (rect.right - rect.left).clamp(1, 65535) as u32;
                let height = (rect.bottom - rect.top).clamp(1, 65535) as u32;
                return DefSubclassProc(
                    h,
                    WM_SIZE,
                    WPARAM(SIZE_RESTORED as usize),
                    LPARAM((width | (height << 16)) as isize),
                );
            }
            return LRESULT(0);
        }
        if m == WM_MOUSEACTIVATE {
            return LRESULT(MA_NOACTIVATE as isize);
        }
        if m == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(h, Some(no_activate), id);
        }
        DefSubclassProc(h, m, w, l)
    }
}
pub fn activate(window: &gpui::Window) -> Result<()> {
    let h = hwnd(window)?;
    unsafe {
        let _ = ShowWindow(h, SW_RESTORE);
        SetForegroundWindow(h).ok()?;
        let _ = SetActiveWindow(h);
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(h));
    }
    Ok(())
}
pub fn show_popup(window: &gpui::Window, anchor: Rect, logical: [f64; 2]) -> Result<Rect> {
    let h = hwnd(window)?;
    unsafe {
        let style = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        SetLastError(WIN32_ERROR(0));
        if SetWindowLongPtrW(
            h,
            GWL_EXSTYLE,
            (style | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) as isize,
        ) == 0
            && GetLastError().0 != 0
        {
            return Err(Error::from_win32());
        }
        SetWindowSubclass(h, Some(no_activate), 1, 0).ok()?;
        let work = util::work_area([anchor.x as i32, anchor.y as i32]);
        SetWindowPos(
            h,
            Some(HWND_TOPMOST),
            work.x as i32,
            work.y as i32,
            1,
            1,
            SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )?;
        let scale = GetDpiForWindow(h).max(96) as f64 / 96.;
        let rect = anchor.clamp_popup(logical[0] * scale, logical[1] * scale, work);
        SetWindowPos(
            h,
            Some(HWND_TOPMOST),
            rect.x as i32,
            rect.y as i32,
            rect.width as i32,
            rect.height as i32,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )?;
        PostMessageW(Some(h), WM_APP + 23, WPARAM(0), LPARAM(0))?;
        Ok(rect)
    }
}
pub fn move_popup(window: &gpui::Window, x: i32, y: i32) -> Result<()> {
    unsafe {
        let h = hwnd(window)?;
        let mut r = RECT::default();
        GetWindowRect(h, &mut r)?;
        let work = util::work_area([x, y]);
        let width = r.right - r.left;
        let height = r.bottom - r.top;
        SetWindowPos(
            h,
            None,
            x.max(work.x as i32)
                .min((work.x + work.width) as i32 - width),
            y.max(work.y as i32)
                .min((work.y + work.height) as i32 - height),
            0,
            0,
            SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
        )
    }
}
pub fn keep_accessible(window: &gpui::Window) -> Result<()> {
    unsafe {
        let h = hwnd(window)?;
        let mut rect = RECT::default();
        GetWindowRect(h, &mut rect)?;
        let work = util::work_area([(rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2]);
        let width = (rect.right - rect.left).min(work.width as i32);
        let height = (rect.bottom - rect.top).min(work.height as i32);
        let x = rect
            .left
            .max(work.x as i32)
            .min((work.x + work.width) as i32 - width);
        let y = rect
            .top
            .max(work.y as i32)
            .min((work.y + work.height) as i32 - height);
        SetWindowPos(h, None, x, y, width, height, SWP_NOACTIVATE | SWP_NOZORDER)
    }
}
fn icon(h: HWND, action: NOTIFY_ICON_MESSAGE) -> Result<()> {
    unsafe {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: h,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY,
            hIcon: LoadIconW(Some(GetModuleHandleW(None)?.into()), PCWSTR(1 as _))
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))?,
            ..Default::default()
        };
        let tip = util::wide("BeeK");
        data.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(action, &data).ok()
    }
}
fn configure(h: HWND) {
    unsafe {
        let _ = UnregisterHotKey(Some(h), 1);
        let (enabled, text) = HOTKEY.get().unwrap().lock().unwrap().clone();
        let parsed = parse_hotkey(&text);
        let ok = !enabled
            || parsed.is_some_and(|(mods, key)| {
                RegisterHotKey(Some(h), 1, mods | MOD_NOREPEAT, key).is_ok()
            });
        HOTKEY_OK.store(ok, Ordering::Release);
        CLIPBOARD_SEQUENCE.store(GetClipboardSequenceNumber(), Ordering::Release);
        let _ = RemoveClipboardFormatListener(h);
        if CLIPBOARD_ENABLED.load(Ordering::Acquire) {
            let _ = AddClipboardFormatListener(h);
        }
    }
}
pub fn parse_hotkey(text: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let mut mods = HOT_KEY_MODIFIERS(0);
    let mut key = None;
    for part in text.split('+') {
        match part.trim().to_ascii_lowercase().as_str() {
            "ctrl" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" => mods |= MOD_WIN,
            p if p.len() == 1 && p.as_bytes()[0].is_ascii_alphabetic() => {
                if key
                    .replace(p.as_bytes()[0].to_ascii_uppercase() as u32)
                    .is_some()
                {
                    return None;
                }
            }
            _ => return None,
        }
    }
    if mods.0 == 0 {
        return None;
    }
    key.map(|k| (mods, k))
}
unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if m == RegisterWindowMessageW(w!("TaskbarCreated")) {
            let _ = icon(h, NIM_ADD);
            return LRESULT(0);
        }
        match m {
            TRAY if l.0 as u32 == WM_LBUTTONUP => send(Event::Settings),
            TRAY if l.0 as u32 == WM_RBUTTONUP => {
                if let Ok(menu) = CreatePopupMenu() {
                    let labels = [
                        w!("Settings"),
                        w!("Read clipboard..."),
                        w!("Restore selection detection"),
                        w!("About / updates"),
                        w!("Restart BeeK"),
                        w!("Quit BeeK"),
                    ];
                    for (i, label) in labels.into_iter().enumerate() {
                        let _ = AppendMenuW(menu, MF_STRING, i + 1, label);
                    }
                    let p = util::cursor();
                    let _ = SetForegroundWindow(h);
                    let selected = TrackPopupMenu(
                        menu,
                        TPM_RETURNCMD | TPM_NONOTIFY,
                        p[0],
                        p[1],
                        None,
                        h,
                        None,
                    )
                    .0;
                    match selected {
                        1 => send(Event::Settings),
                        2 => send(Event::Clipboard),
                        3 => send(Event::Restore),
                        4 => send(Event::About),
                        5 => send(Event::Restart),
                        6 => send(Event::Quit),
                        _ => {}
                    }
                    let _ = PostMessageW(Some(h), WM_NULL, WPARAM(0), LPARAM(0));
                    let _ = DestroyMenu(menu);
                }
            }
            WM_HOTKEY => send(Event::Hotkey),
            WM_CLIPBOARDUPDATE => {
                if CLIPBOARD_ENABLED.load(Ordering::Acquire) && {
                    let sequence = GetClipboardSequenceNumber();
                    sequence != 0 && CLIPBOARD_SEQUENCE.swap(sequence, Ordering::AcqRel) != sequence
                } {
                    let mut pid = 0;
                    if let Ok(owner) = GetClipboardOwner() {
                        GetWindowThreadProcessId(owner, Some(&mut pid));
                    }
                    if pid == 0 || pid != CLIPBOARD_HELPER_PID.load(Ordering::Acquire) {
                        send(Event::ClipboardChanged);
                    }
                }
            }
            WM_TIMER if w.0 == 2 => {
                if !SESSION_NOTIFICATIONS.load(Ordering::Acquire) {
                    SESSION_NOTIFICATIONS.store(
                        WTSRegisterSessionNotification(h, NOTIFY_FOR_THIS_SESSION).is_ok(),
                        Ordering::Release,
                    );
                }
                let active = !LOCKED.load(Ordering::Acquire) && util::desktop_available();
                if DESKTOP_ACTIVE.swap(active, Ordering::AcqRel) != active {
                    SYSTEM_EPOCH.fetch_add(1, Ordering::AcqRel);
                    invalidate(active);
                }
            }
            WM_WTSSESSION_CHANGE => {
                match w.0 as u32 {
                    WTS_SESSION_LOCK | WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => {
                        LOCKED.store(true, Ordering::Release)
                    }
                    WTS_SESSION_UNLOCK | WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => {
                        LOCKED.store(false, Ordering::Release)
                    }
                    _ => {}
                }
                DESKTOP_ACTIVE.store(
                    !LOCKED.load(Ordering::Acquire) && util::desktop_available(),
                    Ordering::Release,
                );
                SYSTEM_EPOCH.fetch_add(1, Ordering::AcqRel);
                invalidate(true);
            }
            WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
                SYSTEM_EPOCH.fetch_add(1, Ordering::AcqRel);
                invalidate(true);
            }
            WM_POWERBROADCAST => {
                if w.0 == 4 {
                    DESKTOP_ACTIVE.store(false, Ordering::Release);
                    SYSTEM_EPOCH.fetch_add(1, Ordering::AcqRel);
                    invalidate(false);
                } else if matches!(w.0, 6 | 7 | 18) {
                    DESKTOP_ACTIVE.store(
                        !LOCKED.load(Ordering::Acquire) && util::desktop_available(),
                        Ordering::Release,
                    );
                    SYSTEM_EPOCH.fetch_add(1, Ordering::AcqRel);
                    invalidate(true);
                }
                return LRESULT(1);
            }
            CONFIG => configure(h),
            WM_CLOSE => {
                let _ = DestroyWindow(h);
            }
            WM_DESTROY => {
                let _ = icon(h, NIM_DELETE);
                PostQuitMessage(0);
            }
            _ => return DefWindowProcW(h, m, w, l),
        }
        LRESULT(0)
    }
}
pub struct Desktop {
    window: usize,
    worker: Option<thread::JoinHandle<()>>,
}
impl Desktop {
    pub fn start(events: mpsc::SyncSender<Event>) -> std::result::Result<Self, String> {
        MAIN_STARTED.store(true, Ordering::Release);
        DESKTOP_ACTIVE.store(util::desktop_available(), Ordering::Release);
        EVENTS.set(events).map_err(|_| "桌面服务已启动")?;
        HOTKEY.get_or_init(|| Mutex::new((false, String::new())));
        CLOCK.get_or_init(Instant::now);
        let (tx, rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || unsafe {
            let init = (|| -> Result<_> {
                let instance = GetModuleHandleW(None)?;
                let class = WNDCLASSW {
                    lpfnWndProc: Some(procedure),
                    hInstance: instance.into(),
                    lpszClassName: w!("BeeKDesktopOwner"),
                    ..Default::default()
                };
                if RegisterClassW(&class) == 0 {
                    return Err(Error::from_win32());
                }
                let h = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("BeeKDesktopOwner"),
                    w!("BeeK desktop owner"),
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
                icon(h, NIM_ADD)?;
                SESSION_NOTIFICATIONS.store(
                    WTSRegisterSessionNotification(h, NOTIFY_FOR_THIS_SESSION).is_ok(),
                    Ordering::Release,
                );
                SetTimer(Some(h), 2, 250, None);
                let mh = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), Some(instance.into()), 0)?;
                let kh =
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), Some(instance.into()), 0)?;
                let foreground = SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                let focus = SetWinEventHook(
                    EVENT_OBJECT_FOCUS,
                    EVENT_OBJECT_FOCUS,
                    None,
                    Some(changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                let selection = SetWinEventHook(
                    EVENT_OBJECT_TEXTSELECTIONCHANGED,
                    EVENT_OBJECT_TEXTSELECTIONCHANGED,
                    None,
                    Some(changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                let location = SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    None,
                    Some(changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                Ok((h, mh, kh, [foreground, focus, selection, location]))
            })();
            match init {
                Ok((h, mh, kh, hooks)) => {
                    let _ = tx.send(Ok(h.0 as usize));
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    let _ = UnhookWindowsHookEx(mh);
                    let _ = UnhookWindowsHookEx(kh);
                    for hook in hooks {
                        if !hook.is_invalid() {
                            let _ = UnhookWinEvent(hook);
                        }
                    }
                    let _ = UnregisterHotKey(Some(h), 1);
                    let _ = KillTimer(Some(h), 2);
                    let _ = WTSUnRegisterSessionNotification(h);
                    let _ = RemoveClipboardFormatListener(h);
                    let _ = icon(h, NIM_DELETE);
                }
                Err(_) => {
                    let _ = tx.send(Err("无法启动通知区或输入监听".to_owned()));
                }
            }
        });
        let window = rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "桌面服务启动超时")??;
        Ok(Self {
            window,
            worker: Some(worker),
        })
    }
    pub fn configure(&self, enabled: bool, hotkey: &str, clipboard: bool) {
        *HOTKEY.get().unwrap().lock().unwrap() = (enabled, hotkey.into());
        CLIPBOARD_ENABLED.store(clipboard, Ordering::Release);
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.window as *mut _)),
                CONFIG,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.window as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        let at = Instant::now();
        while self.worker.as_ref().is_some_and(|w| !w.is_finished())
            && at.elapsed() < Duration::from_millis(500)
        {
            thread::sleep(Duration::from_millis(5));
        }
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            let _ = self.worker.take().unwrap().join();
        }
    }
}

```

## src/platform/windows/speech.rs
```rust
//! One fixed STA voice worker, at most one queued utterance and bounded results.
use std::sync::mpsc;
pub struct Speech {
    input: mpsc::SyncSender<(u64, String)>,
    output: mpsc::Receiver<(u64, Result<(), &'static str>)>,
}
impl Speech {
    pub fn new() -> Self {
        let (input, requests) = mpsc::sync_channel::<(u64, String)>(1);
        let (results, output) = mpsc::sync_channel(2);
        std::thread::spawn(move || unsafe {
            use windows::Win32::{Media::Speech::*, System::Com::*};
            // Initializing SAPI before the UI message loop starts can block
            // COM activation on that not-yet-pumping thread. Activate lazily.
            let Ok(first) = requests.recv() else {
                return;
            };
            let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
            let voice = if initialized {
                CoCreateInstance::<_, ISpVoice>(&SpVoice, None, CLSCTX_INPROC_SERVER).ok()
            } else {
                None
            };
            let mut next = Some(first);
            while let Some((generation, text)) = next.take().or_else(|| requests.recv().ok()) {
                let result = if let Some(voice) = &voice {
                    let words = super::util::wide(text);
                    voice
                        .Speak(
                            windows::core::PCWSTR(words.as_ptr()),
                            SPF_DEFAULT.0 as u32,
                            None,
                        )
                        .map_err(|_| "朗读失败，请检查 Windows 语音和音频设备")
                } else {
                    Err("Windows 语音服务不可用，请检查已安装的语音")
                };
                let _ = results.try_send((generation, result));
            }
            drop(voice);
            if initialized {
                CoUninitialize();
            }
        });
        Self { input, output }
    }
    pub fn speak(&self, generation: u64, text: String) -> Result<(), &'static str> {
        if text.is_empty() || text.chars().count() > 16_384 {
            return Err("原文为空或过长，无法朗读");
        }
        self.input
            .try_send((generation, text))
            .map_err(|_| "朗读繁忙，请稍后重试")
    }
    pub fn poll(&self) -> Option<(u64, Result<(), &'static str>)> {
        self.output.try_recv().ok()
    }
}

```

## src/platform/windows/ui.rs
```rust
use super::{
    input::{self, ClipboardIntent, TextInput},
    native::{self, Event},
    selection::{Coordinator, Outcome},
    store::Store,
    util,
};
use crate::{
    core::{
        policy::TTL_MS,
        protocol::{Rect, Selection, Source, Status},
        settings::{KeyEdit, Settings},
        stream::{self, Mailbox, Pool, Progress},
    },
    icons::IconKind,
};
use gpui::{prelude::*, *};
use std::{
    collections::VecDeque,
    sync::{atomic::Ordering, mpsc, Arc, Mutex},
    time::{Duration, Instant},
};

enum Command {
    Click(Selection),
    Translate(Selection, bool),
    Copy(String, u32),
    Edit(Entity<TextInput>, ClipboardIntent),
    Speak(String),
    Close,
    Save(Settings, KeyEdit),
    Update,
    Drag([i32; 2]),
}
type Tx = mpsc::SyncSender<Command>;
actions!(
    windows_ui,
    [
        NextField,
        PreviousField,
        SaveFields,
        TranslateSnapshot,
        CloseView,
        CopyPreview
    ]
);
struct Host;
impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
struct Bee {
    selection: Selection,
    tx: Tx,
    loading: bool,
}
impl Render for Bee {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let image = if self.loading {
            IconKind::Disabled
        } else {
            IconKind::Default
        }
        .render_image(cx);
        div()
            .id("selection-bee")
            .size_full()
            .cursor_pointer()
            .child(img(image).size_full())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if !this.loading && event.click_count == 1 {
                        this.loading = true;
                        let _ = this.tx.try_send(Command::Click(this.selection.clone()));
                        cx.notify();
                    }
                }),
            )
    }
}
fn button(
    label: &'static str,
    id: &'static str,
    command: impl Fn(&mut Panel) + 'static,
    cx: &mut Context<Panel>,
) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .bg(rgb(0xffd359))
        .text_color(rgb(0x282828))
        .cursor_pointer()
        .child(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if event.click_count > 1 {
                    return;
                }
                command(this);
                cx.notify();
            }),
        )
}
struct Panel {
    active: bool,
    focus: FocusHandle,
    selection: Selection,
    body: String,
    status: String,
    snapshot: bool,
    tx: Tx,
}
impl Panel {
    fn translate(&mut self, _: &TranslateSnapshot, _: &mut Window, cx: &mut Context<Self>) {
        if !self.active {
            self.active = true;
            let _ = self
                .tx
                .try_send(Command::Translate(self.selection.clone(), true));
        }
        cx.notify();
    }
    fn copy(&mut self, _: &CopyPreview, _: &mut Window, _: &mut Context<Self>) {
        if !self.body.is_empty() {
            let _ = self.tx.try_send(Command::Copy(self.body.clone(), unsafe {
                windows::Win32::System::DataExchange::GetClipboardSequenceNumber()
            }));
        }
    }
    fn close(&mut self, _: &CloseView, window: &mut Window, _: &mut Context<Self>) {
        window.remove_window();
    }
}
impl Render for Panel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (pron, body) = stream::split_pron(&self.body);
        #[cfg(feature = "test-fixtures")]
        super::diagnostics::event(
            "panel_layout",
            serde_json::json!({"width":f32::from(window.viewport_size().width),"height":f32::from(window.viewport_size().height)}),
        );
        div()
            .id("translation-card")
            .w_full()
            .h(window.viewport_size().height)
            .p_4()
            .pb(px(64.))
            .relative()
            .rounded_xl()
            .bg(rgb(0xfffdf6))
            .text_color(rgb(0x292929))
            .border_1()
            .border_color(rgb(0xe8dfc9))
            .flex()
            .flex_col()
            .gap_3()
            .key_context("BeeKPreview")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::translate))
            .on_action(cx.listener(Self::close))
            .on_action(cx.listener(Self::copy))
            .child(
                div()
                    .id("drag-header")
                    .flex()
                    .justify_between()
                    .cursor(gpui::CursorStyle::ClosedHand)
                    .child(if self.snapshot {
                        "BeeK · 本地选区快照"
                    } else {
                        "BeeK · 翻译"
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| {
                            let _ = this.tx.try_send(Command::Drag(util::cursor()));
                        }),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x78716a))
                    .child(self.status.clone()),
            )
            .child(
                div()
                    .text_sm()
                    .max_h(px(80.))
                    .overflow_hidden()
                    .child(self.selection.text.clone()),
            )
            .when_some(pron, |d, p| d.child(div().text_sm().child(p)))
            .child(
                div()
                    .id("translation-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .text_sm()
                    .child(body),
            )
            .child(
                div()
                    .flex()
                    .absolute()
                    .bottom(px(16.))
                    .left(px(16.))
                    .right(px(16.))
                    .flex_shrink_0()
                    .h(px(32.))
                    .gap_2()
                    .when(self.snapshot, |d| {
                        d.child(button(
                            "翻译 (Enter)",
                            "translate",
                            |s| {
                                if !s.active {
                                    s.active = true;
                                    let _ = s
                                        .tx
                                        .try_send(Command::Translate(s.selection.clone(), true));
                                }
                            },
                            cx,
                        ))
                    })
                    .child(button(
                        "复制译文",
                        "copy",
                        |s| {
                            if !s.body.is_empty() {
                                let _ = s.tx.try_send(Command::Copy(s.body.clone(),unsafe{windows::Win32::System::DataExchange::GetClipboardSequenceNumber()}));
                            }
                        },
                        cx,
                    ))
                    .child(button(
                        "朗读原文",
                        "speak",
                        |s| {
                            let _ = s.tx.try_send(Command::Speak(s.selection.text.clone()));
                        },
                        cx,
                    ))
                    .child(button(
                        "关闭",
                        "close",
                        |s| {
                            let _ = s.tx.try_send(Command::Close);
                        },
                        cx,
                    )),
            )
    }
}
struct Form {
    _subscriptions: Vec<Subscription>,
    save_revision: Option<(Vec<u64>, [bool; 5])>,
    fields: Vec<Entity<TextInput>>,
    cfg: Settings,
    status: String,
    busy: bool,
    clear: bool,
    tx: Tx,
}
impl Form {
    fn signature(&self, cx: &App) -> (Vec<u64>, [bool; 5]) {
        (
            self.fields.iter().map(|f| f.read(cx).revision()).collect(),
            [
                self.cfg.selection_enabled,
                self.cfg.copy_to_popup,
                self.cfg.hotkey_enabled,
                self.cfg.autostart,
                self.clear,
            ],
        )
    }
    fn next(&mut self, _: &NextField, w: &mut Window, cx: &mut Context<Self>) {
        let n = self
            .fields
            .iter()
            .position(|f| f.focus_handle(cx).is_focused(w))
            .unwrap_or(0);
        self.fields[(n + 1) % self.fields.len()].read(cx).focus(w);
    }
    fn previous(&mut self, _: &PreviousField, w: &mut Window, cx: &mut Context<Self>) {
        let n = self
            .fields
            .iter()
            .position(|f| f.focus_handle(cx).is_focused(w))
            .unwrap_or(0);
        self.fields[(n + self.fields.len() - 1) % self.fields.len()]
            .read(cx)
            .focus(w);
    }
    fn save(&mut self, _: &SaveFields, _: &mut Window, cx: &mut Context<Self>) {
        self.submit(cx);
    }
    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let mut cfg = self.cfg.clone();
        cfg.provider.model = self.fields[1].read(cx).value().trim().into();
        cfg.provider.base_url = self.fields[2].read(cx).value().trim().into();
        cfg.hotkey = self.fields[3].read(cx).value().trim().into();
        if cfg.hotkey_enabled && native::parse_hotkey(&cfg.hotkey).is_none() {
            self.status = "快捷键格式例如 Ctrl+Alt+T；需修饰键和一个英文字母".into();
            cx.notify();
            return;
        }
        if let Err(e) = cfg.validate() {
            self.status = e;
            cx.notify();
            return;
        }
        let key = self.fields[0].read(cx).value();
        let edit = if self.clear {
            KeyEdit::Clear
        } else if key.trim().is_empty() {
            KeyEdit::Keep
        } else {
            KeyEdit::Replace(key)
        };
        if self.tx.try_send(Command::Save(cfg, edit)).is_ok() {
            self.busy = true;
            self.save_revision = Some(self.signature(cx));
            self.status = "正在保存…".into();
            cx.notify();
        }
    }
}
impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let labels = [
            "API Key（留空保留原值；输入后显示为星号）",
            "模型 / 推理接入点",
            "完整 API 地址（chat/completions）",
            "全局快捷键（默认 Ctrl+Alt+T）",
        ];
        let mut view = div()
            .id("settings")
            .size_full()
            .p_6()
            .bg(rgb(0xf8f7f3))
            .text_color(rgb(0x252525))
            .flex()
            .flex_col()
            .gap_3()
            .overflow_y_scroll()
            .key_context("BeeKSettings")
            .on_action(cx.listener(Self::next))
            .on_action(cx.listener(Self::previous))
            .on_action(cx.listener(Self::save))
            .child(div().text_xl().child("BeeK 设置"));
        for (i, label) in labels.into_iter().enumerate() {
            view = view
                .child(div().text_sm().child(label))
                .child(self.fields[i].clone());
        }
        let flags = [
            ("选中文本后显示蜜蜂按钮", self.cfg.selection_enabled),
            ("复制文本后显示（默认关闭）", self.cfg.copy_to_popup),
            ("启用全局快捷键", self.cfg.hotkey_enabled),
            ("登录 Windows 时启动", self.cfg.autostart),
            ("清除已保存的 API Key", self.clear),
        ];
        for (i, (label, on)) in flags.into_iter().enumerate() {
            view = view.child(
                div()
                    .id(i)
                    .cursor_pointer()
                    .text_sm()
                    .child(format!("{} {label}", if on { "☑" } else { "☐" }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |s, _, _, cx| {
                            match i {
                                0 => s.cfg.selection_enabled = !s.cfg.selection_enabled,
                                1 => s.cfg.copy_to_popup = !s.cfg.copy_to_popup,
                                2 => s.cfg.hotkey_enabled = !s.cfg.hotkey_enabled,
                                3 => s.cfg.autostart = !s.cfg.autostart,
                                _ => s.clear = !s.clear,
                            }
                            cx.notify();
                        }),
                    ),
            );
        }
        view.child(
            div()
                .text_sm()
                .text_color(rgb(0x795a22))
                .child(self.status.clone()),
        )
        .when(!native::HOTKEY_OK.load(Ordering::Acquire), |d| {
            d.child("快捷键注册失败，可能被其他应用占用；请修改后保存。")
        })
        .child(
            div()
                .id("save")
                .p_2()
                .rounded_md()
                .bg(rgb(0xffd359))
                .cursor_pointer()
                .child(if self.busy {
                    "保存中…"
                } else {
                    "保存 (Ctrl+S)"
                })
                .on_mouse_down(MouseButton::Left, cx.listener(|s, _, _, cx| s.submit(cx))),
        )
        .child(
            div()
                .id("updates")
                .text_sm()
                .cursor_pointer()
                .child(format!("BeeK {} · 手动检查更新", env!("CARGO_PKG_VERSION")))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, _| {
                        let _ = s.tx.try_send(Command::Update);
                    }),
                ),
        )
        .child(
            div()
                .text_xs()
                .child("仅点击翻译后发送所选文本。剪贴板回退不会模拟 Ctrl+C；不自动下载安装更新。"),
        )
    }
}
fn open_form(cx: &mut App, tx: Tx, cfg: Settings, status: String) -> Option<WindowHandle<Form>> {
    let handle = cx
        .open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("BeeK Settings".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(720.), px(780.)),
                    cx,
                ))),
                ..Default::default()
            },
            |w, cx| {
                let mut fields = Vec::new();
                for (i, (label, value)) in [
                    ("API Key", ""),
                    ("model", cfg.provider.model.as_str()),
                    ("API URL", cfg.provider.base_url.as_str()),
                    ("hotkey", cfg.hotkey.as_str()),
                ]
                .into_iter()
                .enumerate()
                {
                    let field = cx.new(|cx| {
                        let mut f = TextInput::new(cx, label);
                        f.set_value(value, cx);
                        if i == 0 {
                            f.mask();
                        }
                        f
                    });
                    fields.push(field);
                }
                fields[0].read(cx).focus(w);
                cx.new(|cx| {
                    let subscriptions = fields
                        .iter()
                        .map(|field| {
                            cx.subscribe(
                                field,
                                |this: &mut Form, field, event: &ClipboardIntent, cx| {
                                    if this
                                        .tx
                                        .try_send(Command::Edit(field, event.clone()))
                                        .is_err()
                                    {
                                        this.status = "操作队列繁忙，请稍后重试".into();
                                        cx.notify();
                                    }
                                },
                            )
                        })
                        .collect();
                    Form {
                        _subscriptions: subscriptions,
                        save_revision: None,
                        fields,
                        cfg,
                        status,
                        busy: false,
                        clear: false,
                        tx,
                    }
                })
            },
        )
        .map_err(|_| {
            super::diagnostics::event("window_open_failed", serde_json::json!({"kind":"settings"}));
            util::fatal("无法创建设置窗口，请检查显卡驱动后重试");
        })
        .ok()?;
    let _ = handle.update(cx, |_, w, _| {
        let _ = native::keep_accessible(w);
        native::activate(w)
    });
    Some(handle)
}
fn open_bee(cx: &mut App, tx: Tx, s: Selection) -> Option<WindowHandle<Bee>> {
    let anchor = s.rects.last().copied().unwrap_or(Rect {
        x: util::cursor()[0] as f64,
        y: util::cursor()[1] as f64,
        width: 1.,
        height: 1.,
    });
    let w = cx
        .open_window(
            WindowOptions {
                titlebar: None,
                show: false,
                focus: false,
                kind: WindowKind::PopUp,
                is_movable: false,
                is_resizable: false,
                window_background: WindowBackgroundAppearance::Transparent,
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(40.), px(40.)),
                ))),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| Bee {
                    selection: s,
                    tx,
                    loading: false,
                })
            },
        )
        .map_err(|_| {
            super::diagnostics::event(
                "window_open_failed",
                serde_json::json!({"kind":"selection"}),
            )
        })
        .ok()?;
    let _ = w.update(cx, |_, window, _| {
        native::show_popup(window, anchor, [40., 40.])
    });
    Some(w)
}
fn open_panel(cx: &mut App, tx: Tx, s: Selection, snapshot: bool) -> Option<WindowHandle<Panel>> {
    let anchor = s.rects.last().copied().unwrap_or(Rect {
        x: util::cursor()[0] as f64,
        y: util::cursor()[1] as f64,
        width: 1.,
        height: 1.,
    });
    let w = cx
        .open_window(
            WindowOptions {
                titlebar: if snapshot {
                    Some(TitlebarOptions {
                        title: Some("BeeK selection snapshot".into()),
                        ..Default::default()
                    })
                } else {
                    None
                },
                show: snapshot,
                focus: snapshot,
                kind: if snapshot {
                    WindowKind::Normal
                } else {
                    WindowKind::PopUp
                },
                is_movable: snapshot,
                is_resizable: snapshot,
                window_background: WindowBackgroundAppearance::Transparent,
                window_bounds: Some(WindowBounds::Windowed(if snapshot {
                    Bounds::centered(None, size(px(520.), px(420.)), cx)
                } else {
                    Bounds::new(point(px(0.), px(0.)), size(px(520.), px(420.)))
                })),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    if snapshot {
                        window.focus(&focus);
                    }
                    Panel {
                        active: false,
                        focus,
                        status: if s.clipboard_sequence.is_some() {
                            "来自剪贴板 · 仅本地预览，点击翻译后发送".into()
                        } else if snapshot {
                            "已捕获的选区快照 · 点击翻译后发送".into()
                        } else {
                            "正在翻译…".into()
                        },
                        selection: s,
                        body: String::new(),
                        snapshot,
                        tx,
                    }
                })
            },
        )
        .map_err(|_| {
            super::diagnostics::event(
                "window_open_failed",
                serde_json::json!({"kind":"translation"}),
            )
        })
        .ok()?;
    if snapshot {
        let _ = w.update(cx, |_, window, _| {
            let _ = native::keep_accessible(window);
            native::activate(window)
        });
    } else {
        let _ = w.update(cx, |_, window, _| {
            native::show_popup(window, anchor, [520., 420.])
        });
    }
    Some(w)
}
enum Purpose {
    Detect,
    Verify(Selection),
    Preview,
    Clipboard(bool),
    Copy,
}
enum Background {
    Saved(Result<Settings, String>, Option<String>),
    Request(u64, Result<stream::Request, String>),
}
enum Work {
    Save(Settings, Settings, KeyEdit),
    Request(u64, Settings, String),
}
fn selection_status(status: Status) -> &'static str {
    match status {
        Status::TooLong => "选区或剪贴板文本超过 16384 字符，请缩小范围",
        Status::Protected => "该输入区域受保护，未读取文本",
        Status::Empty => "没有可用的文本选区或纯文本剪贴板内容",
        Status::SourceChanged => "来源或剪贴板已变化，请重新选择或重试",
        Status::Busy => "选区检测繁忙，请稍后重试",
        _ => "当前应用未提供可用选区，请使用托盘的剪贴板预览",
    }
}
fn hide<T: 'static>(handle: &mut Option<WindowHandle<T>>, cx: &mut App) {
    if let Some(w) = handle.take() {
        let _ = w.update(cx, |_, w, _| w.remove_window());
    }
}

fn startup_settings(store: &mut Store) -> Result<Settings, String> {
    let cfg = store.load()?;
    if cfg.provider.api_key.trim().is_empty() {
        let mut cfg = cfg;
        cfg.provider.api_key.clear();
        Ok(cfg)
    } else {
        let key = cfg.provider.api_key.clone();
        store.save(&cfg, cfg.clone(), KeyEdit::Replace(key))
    }
}
pub fn run() -> Result<(), String> {
    util::check_platform()?;
    let _installation = super::lifecycle::install_guard()?;
    let restarting = std::env::args().nth(1).as_deref() == Some("--restart-child");
    let (etx, erx) = mpsc::sync_channel(32);
    let instance = if restarting {
        None
    } else {
        match super::lifecycle::Instance::begin(etx.clone())? {
            Some(i) => Some(i),
            None => return Ok(()),
        }
    };
    super::diagnostics::event("startup_phase", serde_json::json!({"phase":"instance"}));
    super::pair::helper()?;
    super::diagnostics::event("startup_phase", serde_json::json!({"phase":"pair"}));
    let store = Arc::new(Mutex::new(Store::open()?));
    let loaded = {
        let mut store = store.lock().unwrap();
        let loaded = startup_settings(&mut store);
        if loaded.is_ok() {
            let _ = store.cleanup();
        }
        loaded
    };
    super::diagnostics::event("startup_phase", serde_json::json!({"phase":"settings"}));
    let mut valid = loaded.is_ok();
    let mut initial_error = loaded.as_ref().err().cloned().unwrap_or_default();
    let initial = loaded.unwrap_or_default();
    let pool = Pool::new();
    let speech = super::speech::Speech::new();
    let (tx, rx) = mpsc::sync_channel(32);
    let (bg_tx, bg_rx) = mpsc::sync_channel(2);
    let (work_tx, work_rx) = mpsc::sync_channel(1);
    let mut cfg = initial;
    let startup_store = store.clone();
    std::thread::spawn(move || {
        while let Ok(work) = work_rx.recv() {
            let result = match work {
                Work::Save(old, next, key) => {
                    let saved = {
                        let mut store = store.lock().unwrap();
                        let saved = store.save(&old, next, key);
                        if saved.is_ok() {
                            let _ = store.cleanup();
                        }
                        saved
                    };
                    let warning = saved
                        .as_ref()
                        .ok()
                        .and_then(|cfg| super::integration::autostart(cfg.autostart).err());
                    Background::Saved(saved, warning)
                }
                Work::Request(g, config, text) => {
                    let key = store.lock().unwrap().key(&config);
                    Background::Request(
                        g,
                        key.map(|key| stream::Request {
                            generation: g,
                            text,
                            key,
                            model: config.provider.model,
                            endpoint: config.provider.base_url,
                        }),
                    )
                }
            };
            if bg_tx.send(result).is_err() {
                break;
            }
        }
    });
    super::diagnostics::event("startup_phase", serde_json::json!({"phase":"gpui"}));
    Application::new().run(move|cx|{
  input::bind(cx);cx.bind_keys([KeyBinding::new("tab",NextField,Some("BeeKSettings")),KeyBinding::new("shift-tab",PreviousField,Some("BeeKSettings")),KeyBinding::new("ctrl-s",SaveFields,Some("BeeKSettings")),KeyBinding::new("enter",TranslateSnapshot,Some("BeeKPreview")),KeyBinding::new("escape",CloseView,Some("BeeKPreview")),KeyBinding::new("ctrl-c",CopyPreview,Some("BeeKPreview"))]);
  let host=match cx.open_window(WindowOptions{titlebar:None,show:false,focus:false,kind:WindowKind::PopUp,window_bounds:Some(WindowBounds::Windowed(Bounds::new(point(px(0.),px(0.)),size(px(1.),px(1.))))),..Default::default()},|_,cx|cx.new(|_|Host)).map_err(|_|()){Ok(host)=>host,Err(_)=>{util::fatal("图形初始化失败，请检查显卡驱动");cx.quit();return;}};
  // A restart child reaches GPUI/GPU window creation before ChildReady. Until
  // ParentExitCommitted and actual parent death it installs no tray or hooks.
  let initialized=(||->Result<_,String>{
   let instance=if restarting{super::lifecycle::child_preflight()?;let committed=startup_settings(&mut startup_store.lock().unwrap());valid=committed.is_ok();initial_error=committed.as_ref().err().cloned().unwrap_or_default();cfg=committed.unwrap_or_default();super::lifecycle::Instance::begin(etx.clone())?.ok_or("重启后会话锁仍被占用")?}else{instance.unwrap()};
   let coordinator=Coordinator::start()?;let desktop=native::Desktop::start(etx)?;desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);Ok((instance,coordinator,desktop))
  })();
  let(mut instance,coordinator,desktop)=match initialized{Ok(v)=>v,Err(e)=>{if !restarting{util::fatal(&e);}cx.quit();return;}};
  let mut form=if std::env::args().any(|a|a=="--autostart"){None}else{open_form(cx,tx.clone(),cfg.clone(),initial_error.clone())};let mut bee:Option<WindowHandle<Bee>>=None;let mut panel:Option<WindowHandle<Panel>>=None;
  cx.spawn(async move|cx|{
   let(_host,_desktop)=(host,desktop);let mut config_valid=valid;let mut epoch=native::EPOCH.load(Ordering::Acquire);let mut trigger=0;let mut id=0;let mut generation=0;let mut pending:Option<(u64,Purpose)>=None;let mut cache:Option<(Selection,Instant)>=None;let mut last_source:Option<Source>=None;let mut mailbox:Option<(u64,Mailbox)>=None;let mut dragging:Option<([i32;2],[i32;2])>=None;let mut saving=false;let mut request_snapshot=false;let mut request_epoch=0;let mut hotkey_requested=false;let mut copy_requested=false;let mut quiet_at=Instant::now();let mut system_epoch=native::SYSTEM_EPOCH.load(Ordering::Acquire);let mut edits:VecDeque<(Entity<TextInput>,ClipboardIntent)>=VecDeque::new();let mut edit_active:Option<(u64,Entity<TextInput>,ClipboardIntent)>=None;let mut failed_copy:Option<Instant>=None;let mut last_applied_edit:Option<(Entity<TextInput>,u64,u64,Instant)>=None;let mut last_settings_status:Option<String>=if initial_error.is_empty(){None}else{Some(initial_error.clone())};
   loop {
    let context=native::SYSTEM_EPOCH.load(Ordering::Acquire);if context!=system_epoch{system_epoch=context;edits.clear();edit_active=None;failed_copy=None;last_applied_edit=None;pending=None;cache=None;generation+=1;pool.cancel_pending();mailbox=None;request_snapshot=false;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);if let Some(f)=form{let _=f.update(cx,|f,w,cx|{let _=native::keep_accessible(w);f.status="系统状态已变化，未完成的剪贴板操作已取消".into();cx.notify();});}});}
    let changed=native::EPOCH.load(Ordering::Acquire);
    if let Some(source)=util::current(changed){last_source=Some(source);}
    if changed!=epoch{epoch=changed;quiet_at=Instant::now();pending=None;cache=None;if !request_snapshot{generation+=1;pool.cancel_pending();mailbox=None;}let _=cx.update(|cx|{hide(&mut bee,cx);if panel.is_some_and(|w|w.update(cx,|p,_,_|!p.snapshot).unwrap_or(false)){hide(&mut panel,cx);}});}
    if panel.is_some_and(|p|p.update(cx,|p,_,_|!p.snapshot&&!util::is_current(p.selection.source)).unwrap_or(false)){generation+=1;pool.cancel_pending();mailbox=None;let _=cx.update(|cx|hide(&mut panel,cx));}
    if cache.as_ref().is_some_and(|(s,at)|at.elapsed()>Duration::from_millis(TTL_MS)||!util::is_current(s.source)){cache=None;let _=cx.update(|cx|hide(&mut bee,cx));}
    let t=native::triggered();if edit_active.is_none()&&edits.is_empty()&&cfg.selection_enabled&&!hotkey_requested&&!copy_requested&&pending.as_ref().is_none_or(|(_,p)|matches!(p,Purpose::Detect))&&t!=trigger&&native::quiet_since(t){trigger=t;if let Some(source)=util::current(epoch){id+=1;coordinator.request(id,source,epoch,util::cursor(),false);pending=Some((id,Purpose::Detect));}}
    while let Ok(event)=erx.try_recv(){match event{
     Event::Settings|Event::About=>{let about=matches!(event,Event::About);let info=format!("BeeK {} · Windows x64 · helper {} · {}",env!("CARGO_PKG_VERSION"),env!("BEEK_BUILD_ID").get(..12).unwrap_or("unknown"),coordinator.snapshot());let _=cx.update(|cx|{hide(&mut bee,cx);let active=form.and_then(|f|f.update(cx,|_,w,_|native::activate(w)).ok());if active.is_none(){form=open_form(cx,tx.clone(),cfg.clone(),if about{info.clone()}else{last_settings_status.clone().unwrap_or_default()});}if about{if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=info;cx.notify();});}}});},
     Event::IpcUnavailable=>{let message="实例通信暂不可用，请从托盘退出后重新启动".to_owned();last_settings_status=Some(message.clone());let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});});},
     Event::Restore=>coordinator.restore(),Event::Quit=>{super::lifecycle::exit_deadline();let _=cx.update(|cx|cx.quit());return;},
     Event::Restart=>{if let Err(e)=instance.restart(){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|s,_,cx|{s.status=e;cx.notify();});});}},
     Event::Hotkey=>{hotkey_requested=true;quiet_at=Instant::now();},
     Event::ClipboardChanged=>{copy_requested=true;quiet_at=Instant::now();},
     Event::Clipboard=>{
      if let Some(source)=util::current(epoch).or(last_source).or_else(||util::source(unsafe{windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()},epoch)){id+=1;let auto=matches!(event,Event::ClipboardChanged);if auto{coordinator.request(id,source,epoch,util::cursor(),true);}else{coordinator.explicit_clipboard(id,source,epoch);}pending=Some((id,Purpose::Clipboard(auto)));}
     }
    }}
    if hotkey_requested&&quiet_at.elapsed()>=Duration::from_millis(150)&&native::parse_hotkey(&cfg.hotkey).is_none_or(|(_,key)|unsafe{windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(key as i32)}>=0){hotkey_requested=false;trigger=native::triggered();if let Some(source)=util::current(epoch){id+=1;coordinator.request(id,source,epoch,util::cursor(),false);pending=Some((id,Purpose::Preview));}}
    if copy_requested&&quiet_at.elapsed()>=Duration::from_millis(150){copy_requested=false;if let Some(source)=util::current(epoch){id+=1;coordinator.request(id,source,epoch,util::cursor(),true);pending=Some((id,Purpose::Clipboard(true)));}}
    if let Some((g,result))=speech.poll(){if g==generation{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status=result.map(|_|"朗读完成").unwrap_or_else(|e|e).into();cx.notify();});});}}
    let restart=instance.restart_events.as_ref().and_then(|rx|rx.try_recv().ok());
    if let Some(event)=restart{match event{
     super::lifecycle::RestartEvent::Ready=>instance.grant_restart(),
     super::lifecycle::RestartEvent::Committed=>{instance.commit_exit();super::lifecycle::exit_deadline();let _=cx.update(|cx|cx.quit());return;},
     super::lifecycle::RestartEvent::Failed(e)=>{instance.restart_events=None;let _=cx.update(|cx|{if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=e;cx.notify();});}else{form=open_form(cx,tx.clone(),cfg.clone(),e);}});}
    }}
    let outcomes={coordinator.result.lock().unwrap().drain(..).collect::<Vec<_>>()};
    for outcome in outcomes{
     let reply=match &outcome{Outcome::Selected(id,_)|Outcome::Empty(id,_)|Outcome::Fault(id,_)|Outcome::Copied(id,_)=>Some(*id),_=>None};
     if edit_active.as_ref().is_some_and(|(id,_,_)|Some(*id)==reply){
      let (_,field,intent)=edit_active.take().unwrap();
      let mut applied=matches!((&intent,&outcome),(ClipboardIntent::Copy{cut:false,..},Outcome::Copied(..)));
      if let Some(f)=form{let _=f.update(cx,|f,w,cx|{
       if f.fields.contains(&field){
        let updated=match (&intent,&outcome){
         (ClipboardIntent::Paste{ticket,..},Outcome::Selected(_,selection))=>field.update(cx,|field,cx|field.paste_value(*ticket,&selection.text,w,cx)).map(|revision|(*ticket,revision)),
         (ClipboardIntent::Copy{ticket,range,cut:true,..},Outcome::Copied(..))=>field.update(cx,|field,cx|field.cut_complete(*ticket,range.clone(),w,cx)).map(|revision|(*ticket,revision)),_=>None
        };
        if let Some((old,revision))=updated{applied=true;last_applied_edit=Some((field.clone(),old,revision,Instant::now()));for(target,intent)in &mut edits{if target==&field{if let ClipboardIntent::Paste{ticket,..}=intent{if *ticket==old{*ticket=revision;}}}}}
       }
       f.status=if applied{"剪贴板操作已完成"}else if matches!(outcome,Outcome::Copied(..)){"已复制；输入位置已变化，未执行剪切"}else{"输入位置、组合文字或剪贴板已变化，操作未完成，请重试"}.into();cx.notify();
      });}
      #[cfg(feature="test-fixtures")]super::diagnostics::event("editor_result",serde_json::json!({"id":reply,"applied":applied}));
      if !matches!(outcome,Outcome::Copied(..)){if let ClipboardIntent::Copy{..}=intent{failed_copy=Some(Instant::now());edits.retain(|(_,e)|!matches!(e,ClipboardIntent::Paste{..}));}}
      continue;
     }
     match outcome{
     Outcome::Selected(reply,s) if pending.as_ref().is_some_and(|(expected,_)|*expected==reply)=>{
      let purpose=pending.take().unwrap().1;
      if s.epoch==epoch&&s.epoch==native::EPOCH.load(Ordering::Acquire)&&(util::is_current(s.source)||matches!(purpose,Purpose::Clipboard(false))){match purpose{
       Purpose::Copy=>{},
       Purpose::Detect=>{cache=Some((s.clone(),Instant::now()));let _=cx.update(|cx|{hide(&mut bee,cx);bee=open_bee(cx,tx.clone(),s);});},
       Purpose::Verify(old)=>if old.same_selection(&s){let _=tx.try_send(Command::Translate(s,false));}else{let _=cx.update(|cx|hide(&mut bee,cx));},
       Purpose::Preview|Purpose::Clipboard(false)=>{generation+=1;pool.cancel_pending();mailbox=None;request_snapshot=false;let _=cx.update(|cx|{hide(&mut bee,cx);hide(&mut panel,cx);panel=open_panel(cx,tx.clone(),s,true);});},
       Purpose::Clipboard(true)=>{cache=Some((s.clone(),Instant::now()));let _=cx.update(|cx|{hide(&mut bee,cx);bee=open_bee(cx,tx.clone(),s);});}
      }}
     },
     Outcome::Copied(reply,_)=>{if pending.as_ref().is_some_and(|(i,_)|*i==reply){pending=None;let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="已复制译文".into();cx.notify();});});}},
     Outcome::Empty(reply,status)=>{if pending.as_ref().is_some_and(|(i,_)|*i==reply){let purpose=pending.take().unwrap().1;let copy=matches!(purpose,Purpose::Copy);let manual=matches!(purpose,Purpose::Preview|Purpose::Clipboard(false));let _=cx.update(|cx|{hide(&mut bee,cx);if copy{if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，剪贴板可能已变化或被占用，请重试".into();cx.notify();});}}if manual{let message=selection_status(status).to_owned();if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});}else{form=open_form(cx,tx.clone(),cfg.clone(),message);}}});}},
     Outcome::Fault(reply,code)=>{let _=code;if pending.as_ref().is_some_and(|(i,_)|*i==reply){let purpose=pending.take().unwrap().1;let copy=matches!(purpose,Purpose::Copy);let manual=matches!(purpose,Purpose::Preview|Purpose::Clipboard(false));if manual{last_settings_status=Some("选区或剪贴板检测超时、繁忙或已取消，请重试".into());}let _=cx.update(|cx|{hide(&mut bee,cx);if copy{if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，请稍后重试".into();cx.notify();});}}if manual{let message=last_settings_status.clone().unwrap();if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});}else{form=open_form(cx,tx.clone(),cfg.clone(),message);}}});}},
     Outcome::Paused=>{let _=cx.update(|cx|{hide(&mut bee,cx);if let Some(f)=form{let _=f.update(cx,|s,_,cx|{s.status="选区检测已暂停，请从托盘手动恢复".into();cx.notify();});}});},_=>{}
    }}
    while let Ok(command)=rx.try_recv(){match command{
     Command::Click(s)=>if cache.as_ref().is_some_and(|(c,at)|c.same_selection(&s)&&at.elapsed()<Duration::from_millis(TTL_MS))&&s.epoch==epoch&&util::is_current(s.source){id+=1;coordinator.request(id,s.source,epoch,util::cursor(),s.clipboard_sequence.is_some());pending=Some((id,Purpose::Verify(s)));},
     Command::Translate(s,snapshot)=>{
      if snapshot{let valid=panel.is_some_and(|p|p.update(cx,|p,_,_|p.snapshot&&p.selection.same_selection(&s)).unwrap_or(false));if !valid{continue;}}
      else if s.epoch!=native::EPOCH.load(Ordering::Acquire)||!util::is_current(s.source){continue;}
      generation+=1;pool.cancel_pending();request_snapshot=snapshot;request_epoch=s.epoch;let g=generation;let config=cfg.clone();let text=s.text.clone();
      let _=cx.update(|cx|{hide(&mut bee,cx);if !snapshot{hide(&mut panel,cx);panel=open_panel(cx,tx.clone(),s,false);}if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=true;p.status="正在翻译…".into();p.body.clear();cx.notify();});}});
      let error=if saving{Some("设置正在保存，请稍后重新选择或重试")}else if !config_valid{Some("配置损坏，恢复并保存设置前不能发送翻译请求")}else if work_tx.try_send(Work::Request(g,config,text)).is_err(){Some("设置操作繁忙，请稍后重试")}else{None};
      if let Some(error)=error{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=false;p.status=error.into();cx.notify();});});}
     },
     Command::Save(next,key) if !saving=>{
      saving=if edit_active.is_some()||!edits.is_empty(){false}else{work_tx.try_send(Work::Save(cfg.clone(),next,key)).is_ok()};if !saving{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.busy=false;f.status=if edit_active.is_some()||!edits.is_empty(){"请等待剪贴板编辑完成后再保存"}else{"设置操作繁忙，请稍后重试"}.into();cx.notify();});});}
     },
     Command::Copy(text,sequence)=>{if let Some(source)=util::source(unsafe{windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow()},epoch){id+=1;coordinator.copy(id,source,epoch,text,sequence);pending=Some((id,Purpose::Copy));}else{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status="复制未完成，请返回普通桌面后重试".into();cx.notify();});});}},
     Command::Edit(field,mut intent)=>{
      #[cfg(feature="test-fixtures")]super::diagnostics::event("editor_intent",match &intent{ClipboardIntent::Paste{ticket,..}=>serde_json::json!({"kind":"paste","ticket":ticket}),ClipboardIntent::Copy{ticket,cut,..}=>serde_json::json!({"kind":if *cut{"cut"}else{"copy"},"ticket":ticket})});if let ClipboardIntent::Paste{ticket,at,..}=&mut intent{if let Some((target,old,new,completed))=&last_applied_edit{if target==&field&&*ticket==*old&&*at<=*completed{*ticket=*new;}}}if edits.len()<4{let ignore=matches!(&intent,ClipboardIntent::Paste{at,..} if failed_copy.is_some_and(|failed|*at<=failed));if !ignore{edits.push_back((field,intent));}else{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status="先前复制未完成，关联粘贴已取消，请重试".into();cx.notify();});});}}else{let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status="剪贴板操作繁忙，请稍后重试".into();cx.notify();});});}},
     Command::Speak(text)=>{let result=speech.speak(generation,text);let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.status=result.map(|_|"正在朗读…").unwrap_or_else(|e|e).into();cx.notify();});});},Command::Close=>{generation+=1;pool.cancel_pending();mailbox=None;let _=cx.update(|cx|{hide(&mut panel,cx);hide(&mut bee,cx);});},
     Command::Update=>{let queued=super::integration::open_updates();let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=if queued{"正在检查 Windows 稳定版更新…"}else{"更新检查繁忙，请稍后重试"}.into();cx.notify();});});},
     Command::Drag(at)=>{if let Some(p)=panel{let mut origin=None;let _=p.update(cx,|p,w,_|{if !p.snapshot{if let Ok(h)=native::hwnd(w){let mut rect=windows::Win32::Foundation::RECT::default();if unsafe{windows::Win32::UI::WindowsAndMessaging::GetWindowRect(h,&mut rect)}.is_ok(){origin=Some([rect.left,rect.top]);}}}});if let Some(origin)=origin{dragging=Some((at,origin));}}},_=>{}
    }}
    if edit_active.is_none(){if let Some((field,intent))=edits.pop_front(){
     let source=form.and_then(|f|f.update(cx,|_,w,_|native::hwnd(w).ok().and_then(|h|util::source(h,epoch))).ok().flatten());
     if let Some(source)=source{id+=1;match &intent{ClipboardIntent::Paste{sequence,..}=>coordinator.paste(id,source,epoch,*sequence),ClipboardIntent::Copy{text,sequence,..}=>coordinator.copy(id,source,epoch,text.clone(),*sequence)}edit_active=Some((id,field,intent));}else{edits.clear();let message="设置窗口已不可用，未完成的输入操作已取消".to_owned();last_settings_status=Some(message.clone());let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=message;cx.notify();});});}
    }}
    while let Ok(message)=bg_rx.try_recv(){match message{
     Background::Saved(result,warning)=>{saving=false;let committed=result.is_ok();let status=match result{Ok(saved)=>{cfg=saved;config_valid=true;_desktop.configure(cfg.hotkey_enabled,&cfg.hotkey,cfg.copy_to_popup);warning.unwrap_or_else(||"已保存".into())},Err(e)=>e};last_settings_status=Some(status.clone());let editing=edit_active.is_some()||!edits.is_empty();let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{let signature=f.save_revision.take();let unchanged=signature.as_ref().is_some_and(|s|s==&f.signature(cx))&&!editing;f.busy=false;f.status=status;if committed{if unchanged{f.cfg=cfg.clone();}else{f.status.push_str("；提交后的新修改尚未保存");}if signature.as_ref().is_some_and(|(_,flags)|flags[4]==f.clear){f.clear=false;}if !editing&&signature.as_ref().is_some_and(|(fields,_)|fields[0]==f.fields[0].read(cx).revision()){f.fields[0].update(cx,|key,cx|key.set_value("",cx));}}cx.notify();});});},
     Background::Request(g,result) if g==generation&&(request_snapshot||request_epoch==native::EPOCH.load(Ordering::Acquire))&&panel.is_some_and(|p|p.update(cx,|_,_,_|()).is_ok())=>match result{Ok(request)=>mailbox=Some((g,pool.submit(request))),Err(e)=>{let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{p.active=false;p.status=e;cx.notify();});});}},_=>{}
    }}
    if let Some(status)=super::integration::update_result(){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{f.status=status;cx.notify();});});}
    if !native::HOTKEY_OK.load(Ordering::Acquire){let _=cx.update(|cx|if let Some(f)=form{let _=f.update(cx,|f,_,cx|{if !f.status.contains("快捷键"){f.status="全局快捷键注册失败（冲突或格式无效），请修改后保存".into();cx.notify();}});});}
    let mut completed=false;
    if let Some((g,m))=&mailbox{if *g==generation{let progress=std::mem::replace(&mut *m.lock().unwrap(),Progress::Idle);completed=matches!(progress,Progress::Done(_));if !matches!(progress,Progress::Idle){let _=cx.update(|cx|if let Some(p)=panel{let _=p.update(cx,|p,_,cx|{match progress{Progress::Idle=>{},Progress::Loading=>{p.status="正在翻译…".into();},Progress::Queued=>{p.status="等待前面的实际请求结束…".into();},Progress::Text(text)=>{p.status="正在翻译…".into();p.body=text;},Progress::Done(Ok(text))=>{p.active=false;p.body=text;p.status="完成".into();},Progress::Done(Err(e))=>{p.active=false;p.status=e;}}cx.notify();});});}}}
    if completed{mailbox=None;}
    if let Some((start,origin))=dragging{if unsafe{windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(1)}>=0{dragging=None;}else{let point=util::cursor();if let Some(p)=panel{let _=p.update(cx,|_,w,_|native::move_popup(w,origin[0]+point[0]-start[0],origin[1]+point[1]-start[1]));}}}
    cx.background_executor().timer(Duration::from_millis(16)).await;
   }
  }).detach();
 });
    Ok(())
}

```

## src/platform/windows/lifecycle.rs
```rust
use super::{
    native::Event,
    util::{self, Handle},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
use windows::{
    core::{BOOL, HRESULT, PCWSTR, PWSTR},
    Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::{Pipes::*, RemoteDesktop::ProcessIdToSessionId, Threading::*, IO::*},
        UI::WindowsAndMessaging::AllowSetForegroundWindow,
    },
};

// Installed payloads live in immutable version directories. This shared read
// handle excludes an installer/uninstaller's exclusive lock for the app lifetime.
pub fn install_guard() -> Result<Option<fs::File>, String> {
    use std::os::windows::fs::OpenOptionsExt;
    let exe = std::env::current_exe().map_err(|_| "程序路径不可用")?;
    let dir = exe.parent().ok_or("程序目录不可用")?;
    if !dir.join("installed.marker").exists() {
        return Ok(None);
    }
    let root = dir
        .parent()
        .filter(|p| p.file_name().is_some_and(|v| v == "versions"))
        .and_then(|p| p.parent())
        .ok_or("安装目录结构无效")?;
    fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(root.join("install.lock"))
        .map(Some)
        .map_err(|_| "安装/卸载正在进行，请完成后重试".into())
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    pid: u32,
    created: u64,
    session: u32,
}
fn identity(pid: u32) -> Result<Identity, String> {
    unsafe {
        let p = Handle(
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
            .map_err(|_| "进程身份不可读")?,
        );
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        GetProcessTimes(p.0, &mut c, &mut e, &mut k, &mut u).map_err(|_| "进程创建时间不可读")?;
        let mut session = 0;
        ProcessIdToSessionId(pid, &mut session).map_err(|_| "进程会话不可读")?;
        Ok(Identity {
            pid,
            created: ((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64,
            session,
        })
    }
}
fn me() -> Result<Identity, String> {
    identity(unsafe { GetCurrentProcessId() })
}
struct Security {
    descriptor: PSECURITY_DESCRIPTOR,
}
impl Security {
    fn new(sid: &str) -> Result<Self, String> {
        unsafe {
            let sddl = util::wide(format!("D:P(A;;GA;;;SY)(A;;GA;;;{sid})"));
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
            .map_err(|_| "无法设置 IPC 访问控制")?;
            Ok(Self { descriptor })
        }
    }
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.0,
            bInheritHandle: BOOL(0),
        }
    }
}
impl Drop for Security {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
        }
    }
}
fn user_sid() -> Result<String, String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|_| "无法读取用户令牌")?;
        let token = Handle(token);
        let mut needed = 0;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut needed);
        if needed == 0 || needed > 64 * 1024 {
            return Err("用户令牌尺寸无效".into());
        }
        let mut bytes = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(bytes.as_mut_ptr() as *mut _),
            needed,
            &mut needed,
        )
        .map_err(|_| "无法读取用户 SID")?;
        let user = &*(bytes.as_ptr() as *const TOKEN_USER);
        let mut sid = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut sid).map_err(|_| "无法格式化用户 SID")?;
        let result = sid.to_string().map_err(|_| "用户 SID 编码错误".to_owned());
        let _ = LocalFree(Some(HLOCAL(sid.0 as *mut _)));
        result
    }
}
struct Pipe(Handle);
impl Pipe {
    fn server(name: &str, sid: &str) -> Result<Self, String> {
        let security = Security::new(sid)?;
        let name = util::wide(name);
        let attributes = security.attributes();
        let h = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                4096,
                4096,
                1000,
                Some(&attributes),
            )
        };
        if h.is_invalid() {
            return Err("无法创建受保护的本地管道".into());
        }
        Ok(Self(Handle(h)))
    }
    fn client(name: &str) -> Result<Self, String> {
        let name = util::wide(name);
        let h = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                0xc0000000,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
        }
        .map_err(|_| "无法连接正在运行的 BeeK")?;
        Ok(Self(Handle(h)))
    }
    fn connect(&self, timeout: u32) -> Result<(), String> {
        unsafe {
            let event =
                Handle(CreateEventW(None, true, false, None).map_err(|_| "IPC 事件不可用")?);
            let mut ov = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            let status = ConnectNamedPipe(self.0 .0, Some(&mut ov));
            if status
                .as_ref()
                .is_err_and(|e| e.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0))
            {
                return Ok(());
            }
            complete(self.0 .0, &mut ov, status, timeout).map(|_| ())
        }
    }
    fn read<T: for<'a> Deserialize<'a>>(&self) -> Result<T, String> {
        self.read_for(1000)
    }
    fn read_for<T: for<'a> Deserialize<'a>>(&self, timeout: u32) -> Result<T, String> {
        unsafe {
            let event =
                Handle(CreateEventW(None, true, false, None).map_err(|_| "IPC 事件不可用")?);
            let mut ov = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            let mut bytes = [0u8; 4097];
            let status = ReadFile(self.0 .0, Some(&mut bytes), None, Some(&mut ov));
            let n = complete(self.0 .0, &mut ov, status, timeout)?;
            if n == 0 || n > 4096 {
                return Err("IPC 消息长度无效".into());
            }
            serde_json::from_slice(&bytes[..n]).map_err(|_| "IPC 消息格式无效".into())
        }
    }
    fn write(&self, value: &impl Serialize) -> Result<(), String> {
        let bytes = serde_json::to_vec(value).map_err(|_| "IPC 消息编码失败")?;
        if bytes.len() > 4096 {
            return Err("IPC 消息过长".into());
        }
        unsafe {
            let event =
                Handle(CreateEventW(None, true, false, None).map_err(|_| "IPC 事件不可用")?);
            let mut ov = OVERLAPPED {
                hEvent: event.0,
                ..Default::default()
            };
            let status = WriteFile(self.0 .0, Some(&bytes), None, Some(&mut ov));
            if complete(self.0 .0, &mut ov, status, 1000)? != bytes.len() {
                return Err("IPC 消息未完整发送".into());
            }
        }
        Ok(())
    }
    fn peer(&self, server: bool) -> Result<Identity, String> {
        let mut pid = 0;
        unsafe {
            if server {
                GetNamedPipeServerProcessId(self.0 .0, &mut pid)
            } else {
                GetNamedPipeClientProcessId(self.0 .0, &mut pid)
            }
        }
        .map_err(|_| "无法核验管道对端进程")?;
        identity(pid)
    }
}
unsafe fn complete(
    h: HANDLE,
    ov: &mut OVERLAPPED,
    status: windows::core::Result<()>,
    timeout: u32,
) -> Result<usize, String> {
    unsafe {
        if let Err(e) = status {
            if e.code() != windows::core::HRESULT::from_win32(ERROR_IO_PENDING.0) {
                return Err("IPC 操作失败".into());
            }
        }
        if WaitForSingleObject(ov.hEvent, timeout) != WAIT_OBJECT_0 {
            let _ = CancelIoEx(h, Some(ov));
            let mut done = 0;
            let _ = GetOverlappedResult(h, ov, &mut done, true);
            return Err("IPC 操作超时".into());
        }
        let mut n = 0;
        GetOverlappedResult(h, ov, &mut n, false).map_err(|_| "IPC 操作中断")?;
        Ok(n as usize)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    version: u32,
    identity: Identity,
    nonce: String,
    exe: PathBuf,
}
#[derive(Serialize, Deserialize)]
enum PublicCommand {
    Ping,
    ActivateSettings,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicRequest {
    version: u32,
    nonce: String,
    client: Identity,
    command: PublicCommand,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicReply {
    version: u32,
    nonce: String,
    server: Identity,
    ok: bool,
}

pub struct Instance {
    mutex: Handle,
    _running: Handle,
    stop: Arc<AtomicBool>,
    listener: Option<thread::JoinHandle<()>>,
    pub restart_events: Option<mpsc::Receiver<RestartEvent>>,
    grant: Option<mpsc::SyncSender<()>>,
    restart_busy: Arc<AtomicBool>,
    restart_cancel: Option<Arc<AtomicBool>>,
    restart_job: Option<Arc<super::process::Jobs>>,
    restart_committed: bool,
    sid: String,
    identity: Identity,
}
impl Instance {
    pub fn begin(events: mpsc::SyncSender<Event>) -> Result<Option<Self>, String> {
        let sid = user_sid()?;
        let own = me()?;
        let security = Security::new(&sid)?;
        let attributes = security.attributes();
        let name = util::wide(format!("Local\\BeeK-{sid}-{}", own.session));
        let mutex = unsafe { CreateMutexW(Some(&attributes), true, PCWSTR(name.as_ptr())) }
            .map_err(|_| "无法取得 BeeK 会话锁")?;
        let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let mutex = Handle(mutex);
        let pipe_name = format!("\\\\.\\pipe\\BeeK-{sid}-{}", own.session);
        let path = util::known_folder(true)?.join(format!("instance-{}.json", own.session));
        if existing {
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                if let Ok(bytes) = fs::read(&path) {
                    if bytes.len() <= 4096 {
                        if let Ok(owner) = serde_json::from_slice::<Owner>(&bytes) {
                            if owner.version == 1
                                && identity(owner.identity.pid).ok() == Some(owner.identity)
                                && owner.identity.session == own.session
                            {
                                if let Ok(pipe) = Pipe::client(&pipe_name) {
                                    if pipe.peer(true)? != owner.identity {
                                        return Err("BeeK 管道服务身份不匹配".into());
                                    }
                                    unsafe {
                                        let _ = AllowSetForegroundWindow(owner.identity.pid);
                                    }
                                    pipe.write(&PublicRequest {
                                        version: 1,
                                        nonce: owner.nonce.clone(),
                                        client: own,
                                        command: PublicCommand::ActivateSettings,
                                    })?;
                                    let response: PublicReply = pipe.read()?;
                                    if response.version == 1
                                        && response.nonce == owner.nonce
                                        && response.server == owner.identity
                                        && response.ok
                                    {
                                        return Ok(None);
                                    }
                                    return Err("BeeK 会话握手失败".into());
                                }
                            }
                        }
                    }
                }
                if Instant::now() >= deadline {
                    return Err("BeeK 已在启动或暂时无响应，请稍后重试".into());
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
        fs::create_dir_all(path.parent().unwrap()).map_err(|_| "无法创建会话目录")?;
        let owner = Owner {
            version: 1,
            identity: own,
            nonce: util::nonce()?,
            exe: std::env::current_exe().map_err(|_| "程序路径不可用")?,
        };
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, serde_json::to_vec(&owner).unwrap())
            .map_err(|_| "会话记录写入失败")?;
        if path.exists() {
            fs::remove_file(&path).map_err(|_| "旧会话记录无法清理")?;
        }
        fs::rename(temporary, &path).map_err(|_| "会话记录提交失败")?;
        // Create the first pipe before reporting readiness. Each connection uses
        // FIRST_PIPE_INSTANCE and verifies the peer PID, creation time and session.
        let first = Pipe::server(&pipe_name, &sid)?;
        let stop = Arc::new(AtomicBool::new(false));
        let shutdown = stop.clone();
        let thread_sid = sid.clone();
        let listener = thread::spawn(move || {
            let mut next = Some(first);
            let mut create_failures = 0u32;
            while !shutdown.load(Ordering::Acquire) {
                let pipe = match next
                    .take()
                    .or_else(|| Pipe::server(&pipe_name, &thread_sid).ok())
                {
                    Some(p) => {
                        create_failures = 0;
                        p
                    }
                    None => {
                        create_failures = create_failures.saturating_add(1);
                        if create_failures == 50 {
                            super::diagnostics::event(
                                "ipc_unavailable",
                                serde_json::json!({"consecutive_failures":create_failures}),
                            );
                            let _ = events.try_send(Event::IpcUnavailable);
                        }
                        thread::sleep(Duration::from_millis(if create_failures >= 50 {
                            1000
                        } else {
                            20
                        }));
                        continue;
                    }
                };
                if pipe.connect(250).is_err() {
                    continue;
                }
                let request = pipe.read::<PublicRequest>();
                if let Ok(request) = request {
                    let valid = request.version == 1
                        && request.nonce == owner.nonce
                        && request.client.session == own.session
                        && pipe.peer(false).ok() == Some(request.client);
                    if valid {
                        if matches!(request.command, PublicCommand::ActivateSettings) {
                            let _ = events.try_send(Event::Settings);
                        }
                        let _ = pipe.write(&PublicReply {
                            version: 1,
                            nonce: owner.nonce.clone(),
                            server: own,
                            ok: true,
                        });
                    }
                }
            }
        });
        let running = Handle(
            unsafe {
                CreateMutexW(
                    Some(&attributes),
                    true,
                    windows::core::w!("Local\\BeeK-Running"),
                )
            }
            .map_err(|_| "无法创建安装协作锁")?,
        );
        Ok(Some(Self {
            mutex,
            _running: running,
            stop,
            listener: Some(listener),
            restart_events: None,
            grant: None,
            restart_busy: Arc::new(AtomicBool::new(false)),
            restart_cancel: None,
            restart_job: None,
            restart_committed: false,
            sid,
            identity: own,
        }))
    }
    pub fn restart(&mut self) -> Result<(), String> {
        if self.restart_events.is_some() || self.restart_busy.load(Ordering::Acquire) {
            return Err("重启正在进行".into());
        }
        let nonce = util::nonce()?;
        let name = format!(
            "\\\\.\\pipe\\BeeK-restart-{}-{}-{nonce}",
            self.sid, self.identity.session
        );
        let pipe = Pipe::server(&name, &self.sid)?;
        let exe = std::env::current_exe().map_err(|_| "程序路径不可用")?;
        let job = super::process::restart_job()?;
        let cancel = Arc::new(AtomicBool::new(false));
        self.restart_cancel = Some(cancel.clone());
        self.restart_job = Some(job.clone());
        self.restart_committed = false;
        self.restart_busy.store(true, Ordering::Release);
        struct Busy(Arc<AtomicBool>);
        impl Drop for Busy {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let busy = Arc::new(Busy(self.restart_busy.clone()));
        let (tx, rx) = mpsc::sync_channel(2);
        let (grant_tx, grant_rx) = mpsc::sync_channel(1);
        let parent = self.identity;
        thread::spawn(move || {
            let _busy = busy;
            let (created, creation) = mpsc::sync_channel(1);
            let spawn_job = job.clone();
            let spawn_cancel = cancel.clone();
            let spawn_busy = _busy.clone();
            let arguments = format!("--restart-child {} {} {nonce}", parent.pid, parent.created);
            thread::spawn(move || {
                let _busy = spawn_busy;
                let result = super::process::RestartChild::launch(
                    &exe,
                    &arguments,
                    spawn_job,
                    &spawn_cancel,
                );
                let _ = created.send(result);
            });
            let mut child = match creation.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(child)) => child,
                Ok(Err(error)) => {
                    let _ = tx.send(RestartEvent::Failed(error));
                    return;
                }
                Err(_) => {
                    cancel.store(true, Ordering::Release);
                    job.kill();
                    let _ = tx.send(RestartEvent::Failed(
                        "重启创建超时，旧实例继续运行；创建槽实际释放前不能重复重试".into(),
                    ));
                    return;
                }
            };
            let child_pid = child.pid;
            let result = (|| -> Result<(), String> {
                pipe.connect(30_000)?;
                let message: RestartMessage = pipe.read()?;
                let actual = pipe.peer(false)?;
                if message.version != 1
                    || message.nonce != nonce
                    || message.process != actual
                    || actual.pid != child_pid
                    || actual.session != parent.session
                    || message.state != RestartState::ChildReady
                {
                    return Err("重启子进程未就绪，旧进程继续运行".into());
                }
                tx.send(RestartEvent::Ready).map_err(|_| "重启已取消")?;
                grant_rx
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| "重启提交超时")?;
                pipe.write(&RestartMessage {
                    version: 1,
                    nonce: nonce.clone(),
                    process: parent,
                    state: RestartState::ParentExitCommitted,
                })?;
                let accepted: RestartMessage = pipe.read()?;
                if accepted.version != 1
                    || accepted.nonce != nonce
                    || accepted.process != actual
                    || accepted.state != RestartState::ChildCommitAccepted
                {
                    return Err("重启提交未确认，旧进程继续运行".into());
                }
                child.commit()?;
                let _ = tx.send(RestartEvent::Committed);
                Ok(())
            })();
            if let Err(e) = result {
                let _ = tx.send(RestartEvent::Failed(e));
            }
        });
        self.restart_events = Some(rx);
        self.grant = Some(grant_tx);
        Ok(())
    }
    pub fn commit_exit(&mut self) {
        self.restart_committed = true;
    }
    pub fn grant_restart(&mut self) {
        if let Some(tx) = self.grant.take() {
            let _ = tx.try_send(());
        }
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        if !self.restart_committed {
            if let Some(cancel) = &self.restart_cancel {
                cancel.store(true, Ordering::Release);
            }
            if let Some(job) = &self.restart_job {
                job.kill();
            }
        }
        self.stop.store(true, Ordering::Release);
        let at = Instant::now();
        while self.listener.as_ref().is_some_and(|t| !t.is_finished())
            && at.elapsed() < Duration::from_millis(1100)
        {
            thread::sleep(Duration::from_millis(5));
        }
        if self.listener.as_ref().is_some_and(|t| t.is_finished()) {
            let _ = self.listener.take().unwrap().join();
        }
        unsafe {
            let _ = ReleaseMutex(self.mutex.0);
        }
    }
}
pub enum RestartEvent {
    Ready,
    Committed,
    Failed(String),
}
#[derive(PartialEq, Eq, Serialize, Deserialize)]
enum RestartState {
    ChildReady,
    ParentExitCommitted,
    ChildCommitAccepted,
    RestartFailed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestartMessage {
    version: u32,
    nonce: String,
    process: Identity,
    state: RestartState,
}

pub fn child_preflight() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--restart-child") {
        return Ok(());
    }
    if args.len() != 5 || args[4].len() != 48 || !args[4].bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("重启参数无效".into());
    }
    let pid = args[2].parse().map_err(|_| "重启父进程无效")?;
    let created: u64 = args[3].parse().map_err(|_| "重启父进程创建时间无效")?;
    let parent = identity(pid)?;
    let own = me()?;
    if parent.created != created || parent.session != own.session {
        return Err("重启父进程身份不匹配".into());
    }
    super::pair::helper()?;
    super::store::Store::open()?.load()?;
    let sid = user_sid()?;
    let pipe = Pipe::client(&format!(
        "\\\\.\\pipe\\BeeK-restart-{sid}-{}-{}",
        own.session, args[4]
    ))?;
    if pipe.peer(true)? != parent {
        return Err("重启管道父进程身份无效".into());
    }
    let parent_handle = Handle(
        unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                false,
                pid,
            )
        }
        .map_err(|_| "重启父进程不可读")?,
    );
    pipe.write(&RestartMessage {
        version: 1,
        nonce: args[4].clone(),
        process: own,
        state: RestartState::ChildReady,
    })?;
    let response: RestartMessage = pipe.read_for(6000)?;
    if response.version != 1
        || response.nonce != args[4]
        || response.process != parent
        || response.state != RestartState::ParentExitCommitted
    {
        return Err("重启未得到父进程提交".into());
    }
    pipe.write(&RestartMessage {
        version: 1,
        nonce: args[4].clone(),
        process: own,
        state: RestartState::ChildCommitAccepted,
    })?;
    if unsafe { WaitForSingleObject(parent_handle.0, 10_000) } != WAIT_OBJECT_0 {
        return Err("旧进程未按时退出，子进程已取消启动".into());
    }
    Ok(())
}
pub fn exit_deadline() {
    // Only bounds an intentional shutdown; never restarts a crash.
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(1900));
        std::process::exit(0);
    });
}

```

## src/platform/windows/mod.rs
```rust
mod diagnostics;
pub mod helper;
mod input;
mod integration;
mod lifecycle;
mod native;
mod pair;
mod process;
mod selection;
mod speech;
mod store;
mod text_model;
mod ui;
mod util;

pub fn run() {
    if std::env::args().nth(1).as_deref() == Some("--purge-user-data") {
        if store::Store::purge_user_data().is_err() {
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--verify-pair") {
        match pair::helper() {
            Ok(_) => println!(
                "{{\"paired\":true,\"build_id\":\"{}\"}}",
                env!("BEEK_BUILD_ID")
            ),
            Err(_) => {
                println!("{{\"paired\":false}}");
                std::process::exit(3);
            }
        }
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--capabilities") {
        println!("{}", capabilities("main"));
        return;
    }
    let mut session = 0;
    if unsafe {
        windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(
            windows::Win32::System::Threading::GetCurrentProcessId(),
            &mut session,
        )
    }
    .is_err()
        || session == 0
    {
        eprintln!("BeeK requires a logged-in interactive Windows desktop.");
        std::process::exit(2);
    }
    diagnostics::initialize();
    diagnostics::event("started", capabilities("main"));
    if let Err(error) = ui::run() {
        util::fatal(&error);
    }
}
pub fn capabilities(binary: &str) -> serde_json::Value {
    serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"build_id":env!("BEEK_BUILD_ID"),"rustc":env!("BEEK_RUSTC_VERSION"),"protocol":crate::core::protocol::VERSION,"binary":binary,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"native_machine":util::native_machine(),"debug_assertions":cfg!(debug_assertions),"test_fixtures":cfg!(feature="test-fixtures")})
}

#[cfg(feature = "test-fixtures")]
pub mod harness;

```
