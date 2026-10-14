// 全局鼠标状态 —— 供「点击弹窗外部关闭」的检测使用
//
// 弹窗是 focus:false 的非激活窗口，点它外面的地方不会产生任何窗口事件（也不能抢焦点），
// 所以「点击框外」只能自己去问系统当前鼠标状态。NSEvent.mouseLocation / pressedMouseButtons
// 是只读查询，不需要新的权限。
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
