//! All third-party UIA calls, including event registration/removal and callbacks,
//! live in this disposable process. Nothing here is linked into the UI thread.
use super::util;
use crate::core::protocol::{self, Query, Rect, Reply, Selection, Status};
use std::{
    io::{self, BufReader},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::*,
    Win32::{
        Foundation::*,
        System::{Com::*, DataExchange::*, Memory::*, Ole::*, Variant::*},
        UI::Accessibility::*,
    },
};

// COM may deliver callbacks on RPC threads while the MTA loop still heartbeats.
// A packed active-count/oldest-start stamp makes those hangs observable too.
static CALLBACKS: AtomicU64 = AtomicU64::new(0);
fn tick() -> u32 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount() }
}
struct Callback;
impl Callback {
    fn enter() -> Self {
        let now = tick();
        let _ = CALLBACKS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
            let count = v as u32;
            if count == u32::MAX {
                std::process::exit(71);
            }
            Some(if count == 0 {
                ((now as u64) << 32) | 1
            } else {
                v + 1
            })
        });
        Self
    }
}
impl Drop for Callback {
    fn drop(&mut self) {
        let _ = CALLBACKS.fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
            Some(if v as u32 <= 1 { 0 } else { v - 1 })
        });
    }
}
fn callback_watchdog() {
    std::thread::spawn(|| loop {
        let active = CALLBACKS.load(Ordering::Acquire);
        if active as u32 > 0 && tick().wrapping_sub((active >> 32) as u32) > 1000 {
            std::process::exit(72);
        }
        std::thread::sleep(Duration::from_millis(20));
    });
}
#[implement(IUIAutomationEventHandler)]
struct Events {
    dirty: Arc<AtomicBool>,
}
impl IUIAutomationEventHandler_Impl for Events_Impl {
    fn HandleAutomationEvent(
        &self,
        _sender: Ref<'_, IUIAutomationElement>,
        _event: UIA_EVENT_ID,
    ) -> Result<()> {
        let _callback = Callback::enter();
        self.dirty.store(true, Ordering::Release);
        Ok(())
    }
}
fn phase(q: &Query, stage: &'static str) {
    #[cfg(feature = "test-fixtures")]
    super::diagnostics::event(
        "provider_phase",
        serde_json::json!({"pid":std::process::id(),"source_pid":q.source.pid,"id":q.id,"stage":stage}),
    );
    #[cfg(not(feature = "test-fixtures"))]
    let _ = (q, stage);
}
struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
struct Array(*mut SAFEARRAY);
impl Drop for Array {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = SafeArrayDestroy(self.0);
            }
        }
    }
}
unsafe fn array<T: Default + Copy>(
    raw: *mut SAFEARRAY,
    kind: VARENUM,
    max: usize,
) -> Result<Vec<T>> {
    unsafe {
        let a = Array(raw);
        if a.0.is_null()
            || SafeArrayGetDim(a.0) != 1
            || SafeArrayGetVartype(a.0)? != kind
            || SafeArrayGetElemsize(a.0) as usize != std::mem::size_of::<T>()
        {
            return Err(Error::from_hresult(HRESULT(0x80070057u32 as i32)));
        }
        let lo = SafeArrayGetLBound(a.0, 1)?;
        let hi = SafeArrayGetUBound(a.0, 1)?;
        let n = (hi as i64 - lo as i64 + 1).max(0) as usize;
        if n > max {
            return Err(Error::from_hresult(HRESULT(0x80070057u32 as i32)));
        }
        let mut out = Vec::with_capacity(n);
        for i in lo..=hi {
            let mut value = T::default();
            SafeArrayGetElement(a.0, &i, &mut value as *mut _ as *mut _)?;
            out.push(value);
        }
        Ok(out)
    }
}
unsafe fn selected(
    element: &IUIAutomationElement,
    q: &Query,
) -> std::result::Result<Selection, Status> {
    unsafe {
        if element.CurrentProcessId().map_err(|_| Status::Failed)? != q.source.pid as i32 {
            return Err(Status::SourceChanged);
        }
        if element
            .CurrentIsPassword()
            .map_err(|_| Status::Protected)?
            .as_bool()
        {
            return Err(Status::Protected);
        }
        let pattern: IUIAutomationTextPattern = element
            .GetCurrentPatternAs(UIA_TextPatternId)
            .map_err(|_| Status::Unsupported)?;
        let ranges = pattern.GetSelection().map_err(|_| Status::Failed)?;
        let count = ranges.Length().map_err(|_| Status::Failed)?;
        if count < 0 || count > 128 {
            return Err(Status::Failed);
        }
        let mut text = String::new();
        let mut rects = Vec::new();
        for i in 0..count {
            let range = ranges.GetElement(i).map_err(|_| Status::Failed)?;
            let raw = range
                .GetText((protocol::MAX_CHARS * 2 + 1) as i32)
                .map_err(|_| Status::Failed)?;
            if raw.len() > protocol::MAX_CHARS * 2 {
                return Err(Status::TooLong);
            }
            let part = String::from_utf16(&raw).map_err(|_| Status::Failed)?;
            if part.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&part);
            if text.chars().count() > protocol::MAX_CHARS {
                return Err(Status::TooLong);
            }
            let raw = range.GetBoundingRectangles().map_err(|_| Status::Failed)?;
            let values = array::<f64>(raw, VT_R8, 4096).map_err(|_| Status::Failed)?;
            if values.len() % 4 != 0 {
                return Err(Status::Failed);
            }
            for v in values.chunks_exact(4) {
                let rect = Rect {
                    x: v[0],
                    y: v[1],
                    width: v[2],
                    height: v[3],
                };
                if rect.valid() {
                    rects.push(rect);
                }
                if rects.len() > 1024 {
                    return Err(Status::Failed);
                }
            }
        }
        if text.trim().is_empty() {
            return Err(Status::Empty);
        }
        if crate::core::prompts::is_url(&text) {
            return Err(Status::Unsupported);
        }
        if q.pointer_anchor {
            if rects.is_empty() {
                rects.push(Rect {
                    x: q.point[0] as f64,
                    y: q.point[1] as f64,
                    width: 1.,
                    height: 1.,
                });
            } else if !q.touches_selection(&rects) {
                return Err(Status::Empty);
            }
        }
        let runtime_id = array::<i32>(
            element.GetRuntimeId().map_err(|_| Status::Failed)?,
            VT_I4,
            128,
        )
        .map_err(|_| Status::Failed)?;
        let s = Selection {
            source: q.source,
            epoch: q.epoch,
            runtime_id,
            text,
            rects,
            clipboard_sequence: None,
        };
        if !s.validate() {
            return Err(Status::Unsupported);
        }
        Ok(s)
    }
}
unsafe fn query(
    uia: &IUIAutomation,
    q: &Query,
) -> std::result::Result<(Selection, IUIAutomationElement), Status> {
    unsafe {
        if !util::is_current(q.source) {
            return Err(Status::SourceChanged);
        }
        phase(q, "focused");
        let focused = uia.GetFocusedElement().map_err(|_| Status::Unsupported)?;
        if focused.CurrentProcessId().ok() == Some(q.source.pid as i32)
            && focused
                .CurrentIsPassword()
                .map_err(|_| Status::Protected)?
                .as_bool()
        {
            return Err(Status::Protected);
        }
        if q.clipboard {
            return clipboard(q).map(|s| (s, focused));
        }
        // Hit-testing can itself block a provider. Only use it when the focused
        // element/ancestors cannot supply a valid selected range.
        let candidates = std::iter::once(focused).chain(q.pointer_points().filter_map(|point| {
            phase(q, "point");
            uia.ElementFromPoint(POINT {
                x: point[0],
                y: point[1],
            })
            .ok()
        }));
        phase(q, "walker");
        let walker = uia.ControlViewWalker().map_err(|_| Status::Unsupported)?;
        let mut last = Status::Unsupported;
        for mut element in candidates {
            phase(q, "ancestry");
            if !inside_window(&element, &walker, q.source.hwnd) {
                continue;
            }
            for _ in 0..8 {
                if element.CurrentProcessId().ok() != Some(q.source.pid as i32) {
                    break;
                }
                phase(q, "selected");
                match selected(&element, q) {
                    Ok(s) => {
                        if !util::is_current(q.source) {
                            return Err(Status::SourceChanged);
                        }
                        return Ok((s, element));
                    }
                    Err(Status::Protected) => return Err(Status::Protected),
                    Err(e) => last = e,
                }
                let Ok(parent) = walker.GetParentElement(&element) else {
                    break;
                };
                element = parent;
            }
        }
        Err(last)
    }
}
unsafe fn inside_window(
    element: &IUIAutomationElement,
    walker: &IUIAutomationTreeWalker,
    root: u64,
) -> bool {
    unsafe {
        let mut node = element.clone();
        for _ in 0..16 {
            if let Ok(h) = node.CurrentNativeWindowHandle() {
                if !h.is_invalid() {
                    return windows::Win32::UI::WindowsAndMessaging::GetAncestor(
                        h,
                        windows::Win32::UI::WindowsAndMessaging::GA_ROOT,
                    ) == util::hwnd(root);
                }
            }
            let Ok(parent) = walker.GetParentElement(&node) else {
                return false;
            };
            node = parent;
        }
        false
    }
}
unsafe fn clipboard(q: &Query) -> std::result::Result<Selection, Status> {
    unsafe {
        let before = GetClipboardSequenceNumber();
        if q.clipboard_sequence != Some(before) {
            return Err(Status::SourceChanged);
        }
        let owner = GetClipboardOwner().ok();
        // Unknown/cross-process owners are left for an explicit local preview; they
        // must never be attributed to the foreground application's live selection.
        if !q.explicit_clipboard
            && owner
                .and_then(|h| util::source(h, q.source.generation))
                .is_none_or(|s| s.pid != q.source.pid || s.created != q.source.created)
        {
            return Err(Status::SourceChanged);
        }
        OpenClipboard(None).map_err(|_| Status::Busy)?;
        struct Clipboard;
        impl Drop for Clipboard {
            fn drop(&mut self) {
                unsafe {
                    let _ = CloseClipboard();
                }
            }
        }
        let _clipboard = Clipboard;
        let handle = GetClipboardData(13).map_err(|_| Status::Empty)?;
        let global = HGLOBAL(handle.0);
        let size = GlobalSize(global);
        if size > (protocol::MAX_CHARS * 2 + 2) * 2 {
            return Err(Status::TooLong);
        }
        if size < 2 || size % 2 != 0 {
            return Err(Status::Failed);
        }
        let ptr = GlobalLock(global) as *const u16;
        if ptr.is_null() {
            return Err(Status::Failed);
        }
        struct Lock(HGLOBAL);
        impl Drop for Lock {
            fn drop(&mut self) {
                unsafe {
                    let _ = GlobalUnlock(self.0);
                }
            }
        }
        let _lock = Lock(global);
        let units = std::slice::from_raw_parts(ptr, size / 2);
        let end = units.iter().position(|n| *n == 0).ok_or(Status::Failed)?;
        let text = String::from_utf16(&units[..end]).map_err(|_| Status::Failed)?;
        if before != GetClipboardSequenceNumber() || owner != GetClipboardOwner().ok() {
            return Err(Status::SourceChanged);
        }
        let selection = Selection {
            source: q.source,
            epoch: q.epoch,
            runtime_id: vec![],
            text,
            rects: vec![],
            clipboard_sequence: Some(before),
        };
        if selection.text.chars().count() > protocol::MAX_CHARS {
            return Err(Status::TooLong);
        }
        if !selection.validate() {
            return Err(Status::Empty);
        }
        Ok(selection)
    }
}

unsafe fn write_clipboard(q: &Query, owner: HWND) -> std::result::Result<u32, Status> {
    unsafe {
        let buffer = q.write_buffer.as_ref().ok_or(Status::Failed)?;
        if q.clipboard_sequence != Some(GetClipboardSequenceNumber()) {
            return Err(Status::SourceChanged);
        }
        if buffer.handle == 0 || buffer.bytes == 0 || buffer.bytes > 256 * 1024 {
            return Err(Status::Failed);
        }
        let handle = util::Handle(HANDLE(buffer.handle as usize as *mut _));
        let view = MapViewOfFile(handle.0, FILE_MAP_READ, 0, 0, buffer.bytes as usize);
        if view.Value.is_null() {
            return Err(Status::Failed);
        }
        struct View(MEMORY_MAPPED_VIEW_ADDRESS);
        impl Drop for View {
            fn drop(&mut self) {
                unsafe {
                    let _ = UnmapViewOfFile(self.0);
                }
            }
        }
        let _view = View(view);
        let bytes = std::slice::from_raw_parts(view.Value as *const u8, buffer.bytes as usize);
        let text = std::str::from_utf8(bytes).map_err(|_| Status::Failed)?;
        if text.contains('\0') {
            return Err(Status::Failed);
        }
        let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let memory = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, text.len() * 2)
            .map_err(|_| Status::Failed)?;
        struct Global(HGLOBAL, bool);
        impl Drop for Global {
            fn drop(&mut self) {
                if !self.1 {
                    unsafe {
                        let _ = GlobalFree(Some(self.0));
                    }
                }
            }
        }
        let mut global = Global(memory, false);
        let ptr = GlobalLock(memory) as *mut u16;
        if ptr.is_null() {
            return Err(Status::Failed);
        }
        std::ptr::copy_nonoverlapping(text.as_ptr(), ptr, text.len());
        let _ = GlobalUnlock(memory);
        OpenClipboard(Some(owner)).map_err(|_| Status::Busy)?;
        struct Clipboard;
        impl Drop for Clipboard {
            fn drop(&mut self) {
                unsafe {
                    let _ = CloseClipboard();
                }
            }
        }
        let _clipboard = Clipboard;
        if q.clipboard_sequence != Some(GetClipboardSequenceNumber()) {
            return Err(Status::SourceChanged);
        }
        EmptyClipboard().map_err(|_| Status::Failed)?;
        SetClipboardData(13, Some(HANDLE(memory.0))).map_err(|_| Status::Failed)?;
        global.1 = true;
        // Windows can advance the sequence again when CloseClipboard publishes
        // the write. Acknowledge the published sequence, with stable ownership,
        // rather than the intermediate value while the clipboard is open.
        drop(_clipboard);
        let after = GetClipboardSequenceNumber();
        if GetClipboardOwner().ok() != Some(owner) || after != GetClipboardSequenceNumber() {
            return Err(Status::SourceChanged);
        }
        Ok(after)
    }
}

pub fn run() {
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 2 && args[1] == "--capabilities" {
        println!("{}", super::capabilities("selection-host"));
        return;
    }
    if args.len() != 3 || args[1] != "--nonce" || args[2].len() != 48 {
        return;
    }
    if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
        return;
    }
    let _com = Com;
    #[cfg(feature = "test-fixtures")]
    super::diagnostics::initialize();
    callback_watchdog();
    let owner = unsafe {
        windows::Win32::UI::WindowsAndMessaging::CreateWindowExW(
            windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("BeeK clipboard owner"),
            windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            None,
        )
    }
    .ok();
    struct Owner(Option<HWND>);
    impl Drop for Owner {
        fn drop(&mut self) {
            if let Some(h) = self.0 {
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(h);
                }
            }
        }
    }
    let _owner = Owner(owner);
    let mut own_sequences = std::collections::VecDeque::new();
    let mut own_after = None;
    let mut own_id = 0;
    let mut uia: Option<IUIAutomation> = None;
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut input = BufReader::new(io::stdin());
        while let Ok(q) = protocol::read_frame::<Query>(&mut input) {
            if tx.send(q).is_err() {
                break;
            }
        }
    });
    let dirty = Arc::new(AtomicBool::new(false));
    let handler: IUIAutomationEventHandler = Events {
        dirty: dirty.clone(),
    }
    .into();
    let mut subscribed: Option<IUIAutomationElement> = None;
    let mut subscribed_window = None;
    let mut previous: Option<Query> = None;
    let mut at = Instant::now();
    let mut output = io::stdout();
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(mut q) => {
                if q.clipboard && q.explicit_clipboard {
                    let current = unsafe { GetClipboardSequenceNumber() };
                    if own_after == Some(current)
                        && q.id > own_id
                        && q.clipboard_sequence
                            .is_some_and(|s| own_sequences.contains(&s))
                    {
                        q.clipboard_sequence = Some(current);
                    } else if own_after != Some(current) {
                        own_sequences.clear();
                        own_after = None;
                    }
                }
                if q.version != protocol::VERSION || q.nonce != args[2] {
                    return;
                }
                #[cfg(feature = "test-fixtures")]
                if std::env::var("BEEK_TEST_HELPER_MODE").as_deref() == Ok("pre-ready-hang") {
                    loop {
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
                let ready = Reply {
                    version: protocol::VERSION,
                    build: env!("BEEK_BUILD_ID").into(),
                    nonce: q.nonce.clone(),
                    id: q.id,
                    source: q.source,
                    selection: None,
                    status: Status::Ready,
                    clipboard_sequence: None,
                };
                if protocol::write_frame(&ready, &mut output).is_err() {
                    break;
                }
                #[cfg(feature = "test-fixtures")]
                match std::env::var("BEEK_TEST_HELPER_MODE")
                    .unwrap_or_default()
                    .as_str()
                {
                    "query-hang" => loop {
                        std::thread::sleep(Duration::from_secs(1));
                    },
                    "clipboard-hang" if q.clipboard => loop {
                        std::thread::sleep(Duration::from_secs(1));
                    },
                    "crash" => std::process::exit(73),
                    "callback-hang" => {
                        let _guard = Callback::enter();
                        loop {
                            std::thread::sleep(Duration::from_secs(1));
                        }
                    }
                    "malformed" => {
                        use std::io::Write;
                        let _ = output.write_all(b"not-json\n");
                        return;
                    }
                    "oversize" => {
                        use std::io::Write;
                        let _ = output.write_all(&vec![b'x'; protocol::MAX_FRAME + 1]);
                        return;
                    }
                    _ => {}
                }
                if q.write_buffer.is_some() {
                    if !q.clipboard || !q.explicit_clipboard {
                        return;
                    }
                    let written = owner
                        .ok_or(Status::Failed)
                        .and_then(|owner| unsafe { write_clipboard(&q, owner) });
                    if let Ok(sequence) = written {
                        if let Some(before) = q.clipboard_sequence {
                            own_sequences.push_back(before);
                        }
                        own_sequences.push_back(sequence);
                        while own_sequences.len() > 16 {
                            own_sequences.pop_front();
                        }
                        own_after = Some(sequence);
                        own_id = q.id;
                    }
                    let status = written
                        .as_ref()
                        .map(|_| Status::ClipboardWritten)
                        .unwrap_or_else(|e| *e);
                    let reply = Reply {
                        version: protocol::VERSION,
                        build: env!("BEEK_BUILD_ID").into(),
                        nonce: q.nonce.clone(),
                        id: q.id,
                        source: q.source,
                        selection: None,
                        status,
                        clipboard_sequence: written.ok(),
                    };
                    if protocol::write_frame(&reply, &mut output).is_err() {
                        break;
                    }
                    previous = Some(q);
                    at = Instant::now();
                    continue;
                }
                dirty.store(false, Ordering::Release);
                if subscribed_window != Some(q.source.hwnd) {
                    if let (Some(old), Some(uia)) = (subscribed.take(), uia.as_ref()) {
                        unsafe {
                            let _ = uia.RemoveAutomationEventHandler(
                                UIA_Text_TextSelectionChangedEventId,
                                &old,
                                &handler,
                            );
                        }
                    }
                    subscribed_window = None;
                }
                let result = if q.clipboard && q.explicit_clipboard {
                    unsafe { clipboard(&q).map(|s| (s, None)) }
                } else {
                    if uia.is_none() {
                        uia =
                            unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
                                .ok();
                    }
                    match uia.as_ref() {
                        Some(uia) => unsafe {
                            // Chromium lazily materializes its accessibility tree
                            // on the first native UIA request. Bounded retries stay
                            // inside the parent's single 1500 ms query deadline.
                            let mut result = query(uia, &q);
                            for _ in 0..3 {
                                if !matches!(result, Err(Status::Unsupported)) {
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(40));
                                result = query(uia, &q);
                            }
                            result.map(|(s, e)| (s, Some(e)))
                        },
                        None => Err(Status::Failed),
                    }
                };
                let (selection, status) = match result {
                    Ok((s, e)) => {
                        if let (Some(_), Some(uia)) = (e, uia.as_ref()) {
                            if !q.clipboard && subscribed.is_none() {
                                if let Ok(root) =
                                    unsafe { uia.ElementFromHandle(util::hwnd(q.source.hwnd)) }
                                {
                                    if unsafe {
                                        uia.AddAutomationEventHandler(
                                            UIA_Text_TextSelectionChangedEventId,
                                            &root,
                                            TreeScope_Subtree,
                                            None,
                                            &handler,
                                        )
                                    }
                                    .is_ok()
                                    {
                                        subscribed = Some(root);
                                        subscribed_window = Some(q.source.hwnd);
                                    }
                                }
                            }
                        }
                        (Some(s), Status::Selected)
                    }
                    Err(e) => (None, e),
                };
                let reply = Reply {
                    version: protocol::VERSION,
                    build: env!("BEEK_BUILD_ID").into(),
                    nonce: q.nonce.clone(),
                    id: q.id,
                    source: q.source,
                    selection,
                    status,
                    clipboard_sequence: None,
                };
                if protocol::write_frame(&reply, &mut output).is_err() {
                    break;
                }
                previous = Some(q);
                at = Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(q) = &previous {
                    if !util::is_current(q.source) {
                        if let (Some(old), Some(uia)) = (subscribed.take(), uia.as_ref()) {
                            unsafe {
                                let _ = uia.RemoveAutomationEventHandler(
                                    UIA_Text_TextSelectionChangedEventId,
                                    &old,
                                    &handler,
                                );
                            }
                            subscribed_window = None;
                        }
                    }
                    let changed = dirty.swap(false, Ordering::AcqRel);
                    if changed || at.elapsed() >= Duration::from_millis(500) {
                        let reply = Reply {
                            version: protocol::VERSION,
                            build: env!("BEEK_BUILD_ID").into(),
                            nonce: q.nonce.clone(),
                            id: q.id,
                            source: q.source,
                            selection: None,
                            clipboard_sequence: None,
                            status: if changed {
                                Status::Changed
                            } else {
                                Status::Alive
                            },
                        };
                        if protocol::write_frame(&reply, &mut output).is_err() {
                            break;
                        }
                        at = Instant::now();
                    }
                }
            }
        }
    }
    if let (Some(old), Some(uia)) = (subscribed, uia.as_ref()) {
        unsafe {
            let _ = uia.RemoveAutomationEventHandler(
                UIA_Text_TextSelectionChangedEventId,
                &old,
                &handler,
            );
        }
    }
}
