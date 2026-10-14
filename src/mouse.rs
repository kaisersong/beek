// 全局鼠标状态 —— 供「点击弹窗外部关闭」和「拖动弹窗」两处使用
//
// 弹窗是 focus:false 的非激活窗口，点它外面的地方不会产生任何窗口事件（也不能抢焦点），
// 所以「点击框外」只能自己去问系统当前鼠标状态。NSEvent.mouseLocation / pressedMouseButtons
// 是只读查询，不需要新的权限。
//
// 「拖动弹窗」为什么也走这里：gpui 0.2.2 在 macOS 上没有任何移动窗口的 API
// （Div::window_control_area 在 mac 平台上是空实现），所以拖动只能自己算偏移、
// 再对 AppKit 的 NSPanel 调 setFrameTopLeftPoint（见 menubar::popup_window_move）。
use cocoa::foundation::NSPoint;
use objc::{class, msg_send, sel, sel_impl};

/// 返回 (x, y, 按下的按键掩码)。
/// x/y 是 Cocoa 屏幕坐标（主屏左下角为原点、y 向上），与 GPUI 的左上原点坐标系相差一次 y 翻转。
pub fn state() -> (f64, f64, i64) {
    unsafe {
        let loc: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        let buttons: i64 = msg_send![class!(NSEvent), pressedMouseButtons];
        (loc.x, loc.y, buttons)
    }
}
