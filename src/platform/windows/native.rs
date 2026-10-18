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
static LAST_ACTIVITY: AtomicU64 = AtomicU64::new(0);
static CLOCK: OnceLock<Instant> = OnceLock::new();
static DRAG: AtomicBool = AtomicBool::new(false);
static MOUSE_INPUT: AtomicBool = AtomicBool::new(false);
static MOUSE_AT: AtomicU64 = AtomicU64::new(0);
static MOUSE_ROOT: AtomicU64 = AtomicU64::new(0);
static MOUSE_PID: AtomicU32 = AtomicU32::new(0);
static MOUSE_POINT: AtomicU64 = AtomicU64::new(0);
static MOUSE_END_POINT: AtomicU64 = AtomicU64::new(0);
static MOUSE_DOWN_ROOT: AtomicU64 = AtomicU64::new(0);
static MOUSE_DOWN_PID: AtomicU32 = AtomicU32::new(0);
static MOUSE_DOWN_POINT: AtomicU64 = AtomicU64::new(0);
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
    let now = CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1;
    LAST_ACTIVITY.fetch_max(now, Ordering::AcqRel);
    EPOCH.fetch_add(1, Ordering::AcqRel);
    if trigger {
        TRIGGER.store(now, Ordering::Release);
    }
}
pub fn selection_changed() {
    invalidate(true);
}
pub fn triggered() -> u64 {
    TRIGGER.load(Ordering::Acquire)
}
fn selection_settled(now: u64, trigger: u64, activity: u64, dragging: bool) -> bool {
    trigger > 0 && !dragging && now.saturating_sub(activity.max(trigger)) >= 120
}
pub fn quiet_since(trigger: u64) -> bool {
    // A pause while the mouse is held is not a completed selection. Debounce
    // caret/focus updates too, rather than starting a query from an older trigger.
    selection_settled(
        CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1,
        trigger,
        LAST_ACTIVITY.load(Ordering::Acquire),
        DRAG.load(Ordering::Acquire),
    )
}
pub struct MouseAnchor {
    pub point: [i32; 2],
    pub end: Option<[i32; 2]>,
}
pub fn mouse_anchor(source: crate::core::protocol::Source) -> Option<MouseAnchor> {
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
    let end = MOUSE_END_POINT.load(Ordering::Acquire);
    Some(MouseAnchor {
        point: [(point >> 32) as u32 as i32, point as u32 as i32],
        end: (end != point).then_some([(end >> 32) as u32 as i32, end as u32 as i32]),
    })
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
            let pressed = w.0 as u32 == WM_LBUTTONUP && DRAG.swap(false, Ordering::AcqRel);
            if external(h) {
                match w.0 as u32 {
                    WM_LBUTTONDOWN => {
                        let root = GetAncestor(h, GA_ROOT);
                        let mut pid = 0;
                        GetWindowThreadProcessId(root, Some(&mut pid));
                        MOUSE_DOWN_ROOT.store(root.0 as u64, Ordering::Release);
                        MOUSE_DOWN_PID.store(pid, Ordering::Release);
                        MOUSE_DOWN_POINT.store(
                            ((data.pt.x as u32 as u64) << 32) | data.pt.y as u32 as u64,
                            Ordering::Release,
                        );
                        DRAG.store(true, Ordering::Release);
                        invalidate(false);
                    }
                    // Only finish a selection gesture that began in an external
                    // window. A click on our bee removes it on mousedown; its
                    // mouseup can then hit the source underneath. That release
                    // must not invalidate the translation just opened by the click.
                    WM_LBUTTONUP if pressed => {
                        let root = GetAncestor(h, GA_ROOT);
                        let mut pid = 0;
                        GetWindowThreadProcessId(root, Some(&mut pid));
                        MOUSE_ROOT.store(root.0 as u64, Ordering::Release);
                        MOUSE_PID.store(pid, Ordering::Release);
                        // A browser drag may end well beyond the selected glyphs.
                        // Validate against where the gesture began, but only if
                        // both ends belong to the same source window/process.
                        let point = if pressed
                            && MOUSE_DOWN_ROOT.load(Ordering::Acquire) == root.0 as u64
                            && MOUSE_DOWN_PID.load(Ordering::Acquire) == pid
                        {
                            MOUSE_DOWN_POINT.load(Ordering::Acquire)
                        } else {
                            ((data.pt.x as u32 as u64) << 32) | data.pt.y as u32 as u64
                        };
                        MOUSE_POINT.store(point, Ordering::Release);
                        MOUSE_END_POINT.store(
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
unsafe extern "system" fn window_adapter(
    h: HWND,
    m: u32,
    w: WPARAM,
    l: LPARAM,
    id: usize,
    no_activate: usize,
) -> LRESULT {
    unsafe {
        // SetWindowPos called while GPUI owns a Window may synchronously invoke
        // on_resize before that Window can be borrowed again. Re-deliver the
        // current client size on the native event loop, after the borrow ends.
        if m == WM_APP + 23 {
            let mut rect = RECT::default();
            if !IsIconic(h).as_bool() && GetClientRect(h, &mut rect).is_ok() {
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
        // GPUI's queued resize uses SetWindowPos without NOZORDER/NOACTIVATE.
        // Keep selection surfaces above their source without taking focus.
        if no_activate != 0 && m == WM_WINDOWPOSCHANGING {
            let position = &mut *(l.0 as *mut WINDOWPOS);
            position.hwndInsertAfter = HWND_TOPMOST;
            position.flags = (position.flags | SWP_NOACTIVATE) & !SWP_NOZORDER;
        }
        // GPUI applies normal-window client insets even to PopUp windows.
        // Our transparent floating surfaces own every pixel, including the edge.
        if no_activate != 0 && m == WM_NCCALCSIZE && w.0 != 0 {
            return LRESULT(0);
        }
        if no_activate != 0 && m == WM_GETMINMAXINFO {
            let info = &mut *(l.0 as *mut MINMAXINFO);
            info.ptMinTrackSize = POINT { x: 1, y: 1 };
            return LRESULT(0);
        }
        if m == WM_MOUSEACTIVATE && no_activate != 0 {
            return LRESULT(MA_NOACTIVATE as isize);
        }
        if m == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(h, Some(window_adapter), id);
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
// Physical coordinates throughout; scale only logical design dimensions. Keep
// the card clear of the selected line, including on monitors with negative origins.
fn popup_rect(anchor: Rect, logical: [f64; 2], work: Rect, scale: f64, selection: bool) -> Rect {
    let margin = 8. * scale;
    let width = (logical[0] * scale).min((work.width - margin * 2.).max(1.));
    let mut height = (logical[1] * scale).min((work.height - margin * 2.).max(1.));
    let left = work.x + margin;
    let top = work.y + margin;
    let right = (work.x + work.width - margin - width).max(left);
    let x = if selection {
        anchor.x + anchor.width - width
    } else {
        anchor.x
    };
    let below = anchor.y + anchor.height + 8. * scale;
    let above_space = (anchor.y - 8. * scale - top).max(1.);
    let below_space = (work.y + work.height - margin - below).max(1.);
    // Match macOS: choose the side using minimum usable space, then grow on
    // that side. Cap height and scroll instead of jumping across the selection.
    let y = if selection {
        let above = anchor.y - height;
        if above >= top {
            above
        } else {
            below
        }
    } else if below_space >= 80. * scale || below_space >= above_space {
        height = height.min(below_space);
        below
    } else {
        height = height.min(above_space);
        anchor.y - 8. * scale - height
    };
    let bottom = (work.y + work.height - margin - height).max(top);
    Rect {
        x: x.clamp(left, right),
        y: y.clamp(top, bottom),
        width,
        height,
    }
}

pub fn show_selection(window: &gpui::Window, anchor: Rect, diameter: f64) -> Result<Rect> {
    show_floating(window, anchor, [diameter, diameter], true)
}
pub fn show_popup(window: &gpui::Window, anchor: Rect, logical: [f64; 2]) -> Result<Rect> {
    show_floating(window, anchor, logical, false)
}
fn show_floating(
    window: &gpui::Window,
    anchor: Rect,
    logical: [f64; 2],
    selection: bool,
) -> Result<Rect> {
    let h = hwnd(window)?;
    unsafe {
        let window_style = GetWindowLongPtrW(h, GWL_STYLE) as u32;
        SetWindowLongPtrW(
            h,
            GWL_STYLE,
            ((window_style & !(WS_CAPTION.0 | WS_THICKFRAME.0 | WS_SYSMENU.0)) | WS_POPUP.0)
                as isize,
        );
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
        SetWindowSubclass(h, Some(window_adapter), 1, 1).ok()?;
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
        let rect = popup_rect(anchor, logical, work, scale, selection);
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
/// Resize without activating or stealing the source selection. Once dragged,
/// preserve the user's position instead of snapping back to the source anchor.
pub fn resize_popup(window: &gpui::Window, anchor: Option<Rect>, logical: [f64; 2]) -> Result<()> {
    let h = hwnd(window)?;
    unsafe {
        let mut current = RECT::default();
        GetWindowRect(h, &mut current)?;
        let scale = GetDpiForWindow(h).max(96) as f64 / 96.;
        let work = util::work_area(
            anchor
                .map(|a| [a.x as i32, a.y as i32])
                .unwrap_or([current.left, current.top]),
        );
        let rect = if let Some(anchor) = anchor {
            popup_rect(anchor, logical, work, scale, false)
        } else {
            let width = (logical[0] * scale).min(work.width);
            let height = (logical[1] * scale).min(work.height);
            Rect {
                x: (current.left as f64).clamp(work.x, work.x + work.width - width),
                y: (current.top as f64).clamp(work.y, work.y + work.height - height),
                width,
                height,
            }
        };
        let (x, y, width, height) = (
            rect.x.round() as i32,
            rect.y.round() as i32,
            rect.width.round() as i32,
            rect.height.round() as i32,
        );
        if (
            current.left,
            current.top,
            current.right - current.left,
            current.bottom - current.top,
        ) != (x, y, width, height)
        {
            SetWindowPos(h, None, x, y, width, height, SWP_NOACTIVATE | SWP_NOZORDER)?;
            PostMessageW(Some(h), WM_APP + 23, WPARAM(0), LPARAM(0))?;
        }
    }
    Ok(())
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
        let no_activate =
            (GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_NOACTIVATE.0 != 0) as usize;
        SetWindowSubclass(h, Some(window_adapter), 1, no_activate).ok()?;
        SetWindowPos(h, None, x, y, width, height, SWP_NOACTIVATE | SWP_NOZORDER)?;
        PostMessageW(Some(h), WM_APP + 23, WPARAM(0), LPARAM(0))
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

#[cfg(test)]
mod popup_tests {
    use super::*;
    #[test]
    fn paused_drag_waits_for_release_and_final_caret_update() {
        // Selection notifications can arrive during a long drag. Even a long
        // pause must not consume its trigger before the user's mouseup.
        for now in [250, 800, 2000] {
            assert!(!selection_settled(now, 100, 100, true));
        }
        assert!(!selection_settled(2050, 2000, 2000, false));
        // The final caret update arrives after mouseup, so settle from that
        // event rather than from the older selection notification.
        assert!(!selection_settled(2150, 2000, 2100, false));
        assert!(selection_settled(2220, 2000, 2100, false));
    }
    #[test]
    fn keyboard_or_double_click_still_settles_without_a_drag() {
        assert!(!selection_settled(119, 1, 1, false));
        assert!(selection_settled(121, 1, 1, false));
        assert!(!selection_settled(1000, 0, 0, false));
    }
    #[test]
    fn selection_matches_mac_size_and_right_alignment_at_high_dpi() {
        let work = Rect {
            x: -1920.,
            y: 0.,
            width: 1920.,
            height: 1080.,
        };
        let anchor = Rect {
            x: -1000.,
            y: 400.,
            width: 200.,
            height: 20.,
        };
        let rect = popup_rect(anchor, [24., 24.], work, 1.5, true);
        assert_eq!(
            rect,
            Rect {
                x: -836.,
                y: 364.,
                width: 36.,
                height: 36.
            }
        );
    }
    #[test]
    fn growing_card_moves_above_selection_near_bottom() {
        let work = Rect {
            x: 0.,
            y: 0.,
            width: 1920.,
            height: 1080.,
        };
        let anchor = Rect {
            x: 300.,
            y: 1000.,
            width: 200.,
            height: 20.,
        };
        let rect = popup_rect(anchor, [380., 320.], work, 1., false);
        assert_eq!(rect.y + rect.height, anchor.y - 8.);
        assert_eq!(rect.width, 380.);
    }
    #[test]
    fn growing_card_keeps_its_side_and_scrolls_in_remaining_space() {
        let work = Rect {
            x: 0.,
            y: 0.,
            width: 1920.,
            height: 1080.,
        };
        let anchor = Rect {
            x: 300.,
            y: 900.,
            width: 200.,
            height: 20.,
        };
        let short = popup_rect(anchor, [380., 100.], work, 1., false);
        let long = popup_rect(anchor, [380., 320.], work, 1., false);
        assert_eq!(short.y, long.y);
        assert_eq!(long.height, 144.);
        assert!(long.y >= anchor.y + anchor.height);
    }
    #[test]
    fn card_stays_within_small_work_area() {
        let work = Rect {
            x: -800.,
            y: -600.,
            width: 300.,
            height: 240.,
        };
        let anchor = Rect {
            x: -510.,
            y: -370.,
            width: 5.,
            height: 5.,
        };
        let rect = popup_rect(anchor, [380., 320.], work, 2., false);
        assert!(rect.x >= work.x && rect.y >= work.y);
        assert!(rect.x + rect.width <= work.x + work.width);
        assert!(rect.y + rect.height <= work.y + work.height);
    }
}
