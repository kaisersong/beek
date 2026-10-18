# R5 唯一 Minor 关闭

已把同类尺寸同步扩展到 keep_accessible。子类 userdata 区分普通窗口与 WS_EX_NOACTIVATE 弹窗，仅后者拦截WM_MOUSEACTIVATE。正常Settings/快照不会失去正常激活。最小化时跳过合成SIZE_RESTORED；WM_NCDESTROY注销。show_popup和keep_accessible都post私有消息，回调当下读取客户区大小。请复核并明确该Minor是否CLOSED、是否有限PASS且无未关闭源码问题；正式G5环境仍未通过。此轮只看附代码，不执行工具、不展开已关闭条目。

```rust
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
        let no_activate =
            (GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 & WS_EX_NOACTIVATE.0 != 0) as usize;
        SetWindowSubclass(h, Some(window_adapter), 1, no_activate).ok()?;
        SetWindowPos(h, None, x, y, width, height, SWP_NOACTIVATE | SWP_NOZORDER)?;
        PostMessageW(Some(h), WM_APP + 23, WPARAM(0), LPARAM(0))
    }
}

```
