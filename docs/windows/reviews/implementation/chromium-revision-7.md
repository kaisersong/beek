# R7 Chromium 鼠标选区与点击复核

请对本次增量做对抗性源码复审。背景：用户报告Notepad可以，Chrome不弹按钮；fresh Chrome154普通段落双击及贴近文本拖动可显示/点击卡片；原eaa621版在同一公开普通段落，从文字起拖到右侧空白（鼠标松开远离glyph）时实际选中内容但无按钮，截图及selection_reply保留。父侧锚点传mouseup，helper要求距任一选区rect<=20，导致合法drag被拒绝。另点击复核原来用当前鼠标（蜜蜂按钮内）作UIA fallback点，当focused无TextPattern时无法命中源文字。

修复：DOWN记录root/PID/point；UP在同root/PID且确有未释放DOWN时使用起点，否则仍用结束点；任何UP（包括落在本进程）清DRAG，避免遗留。500ms锚点时效、source identity/epoch、UIA新鲜查询、protected过滤和点击前零联网全部保留。点击复核改以缓存选区最后rect中心作fallback点，仍必须fresh same_selection才翻译，未用缓存跳过复核。

测试驱动增加真正双击/drag动作，仅接受公开fixture前台窗口与窗口内坐标。实际回归正在运行，不能当成完成。不要执行工具或修改文件。请找可触发缺陷，或给有限PASS；不批准G5发布。无需重审旧范围。

## native鼠标路径
```rust
static DRAG: AtomicBool = AtomicBool::new(false);
static MOUSE_INPUT: AtomicBool = AtomicBool::new(false);
static MOUSE_AT: AtomicU64 = AtomicU64::new(0);
static MOUSE_ROOT: AtomicU64 = AtomicU64::new(0);
static MOUSE_PID: AtomicU32 = AtomicU32::new(0);
static MOUSE_POINT: AtomicU64 = AtomicU64::new(0);
static MOUSE_DOWN_ROOT: AtomicU64 = AtomicU64::new(0);
static MOUSE_DOWN_PID: AtomicU32 = AtomicU32::new(0);
static MOUSE_DOWN_POINT: AtomicU64 = AtomicU64::new(0);
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
                    WM_LBUTTONUP => {
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

```
## 点击路径
```rust
     Command::Click(s)=>if cache.as_ref().is_some_and(|(c,at)|c.same_selection(&s)&&at.elapsed()<Duration::from_millis(TTL_MS))&&s.epoch==epoch&&util::is_current(s.source){id+=1;let point=s.rects.last().map(|r|[(r.x+r.width/2.) as i32,(r.y+r.height/2.) as i32]).unwrap_or_else(util::cursor);coordinator.request(id,s.source,epoch,point,s.clipboard_sequence.is_some());pending=Some((id,Purpose::Verify(s)));},
```
## helper新鲜读取和保护
```rust
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
            } else if !rects.iter().any(|r| {
                q.point[0] as f64 >= r.x - 20.
                    && q.point[0] as f64 <= r.x + r.width + 20.
                    && q.point[1] as f64 >= r.y - 20.
                    && q.point[1] as f64 <= r.y + r.height + 20.
            }) {
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
        let candidates = std::iter::once(focused).chain(
            std::iter::once_with(|| {
                phase(q, "point");
                uia.ElementFromPoint(POINT {
                    x: q.point[0],
                    y: q.point[1],
                })
                .ok()
            })
            .flatten(),
        );
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

```
