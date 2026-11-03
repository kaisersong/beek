//! CoreGraphics supplies global origins that GPUI 0.2.2's MacDisplay omits.
use beek::core::display_geometry::{self, Rect};
use cocoa::base::{id, NO};
use cocoa::foundation::{NSPoint, NSRect};
use objc::{msg_send, sel, sel_impl};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[link(name="CoreGraphics",kind="framework")]
extern "C" {
    fn CGGetActiveDisplayList(max:u32, ids:*mut u32, count:*mut u32)->i32;
    fn CGDisplayBounds(display:u32)->NSRect;
    fn CGMainDisplayID()->u32;
}

#[derive(Clone,Copy,Debug)]
pub struct Screen { pub id:u32, pub bounds:Rect }

pub fn primary_height()->f64 { unsafe { CGDisplayBounds(CGMainDisplayID()).size.height } }

pub fn screens()->Vec<Screen> {
    let mut ids=[0u32;32];let mut count=0;
    let status=unsafe {CGGetActiveDisplayList(ids.len() as u32,ids.as_mut_ptr(),&mut count)};
    if status!=0 || count==0 {ids[0]=unsafe{CGMainDisplayID()};count=1;}
    ids[..(count as usize).min(ids.len())].iter().map(|&id|{
        let r=unsafe {CGDisplayBounds(id)};
        Screen{id,bounds:Rect{x:r.origin.x,y:r.origin.y,w:r.size.width,h:r.size.height}}
    }).filter(|s|s.bounds.w>0. && s.bounds.h>0.).collect()
}

pub fn screen_at(anchor:Rect)->Screen {
    let screens=screens();let bounds=screens.iter().map(|s|s.bounds).collect::<Vec<_>>();
    display_geometry::screen_for(&bounds,anchor).and_then(|i|screens.get(i).copied())
        .unwrap_or(Screen{id:unsafe{CGMainDisplayID()},bounds:Rect{x:0.,y:0.,w:1920.,h:1080.}})
}

/// Borrow the GPUI-owned view/window only for this main-thread operation.
pub fn position_window(window:&gpui::Window,x:f64,global_top:f64) {
    if let Ok(handle)=HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::AppKit(handle)=handle.as_raw() {
            unsafe {
                let view=handle.ns_view.as_ptr() as id;
                let native:id=msg_send![view,window];
                let mut frame:NSRect=msg_send![native,frame];
                frame.origin=NSPoint::new(x,display_geometry::cocoa_y(primary_height(),global_top)-frame.size.height);
                let _:()=msg_send![native,setFrame:frame display:NO];
            }
        }
    }
}

pub fn window_rect(window:&gpui::Window)->Option<Rect> {
    let handle=HasWindowHandle::window_handle(window).ok()?;
    if let RawWindowHandle::AppKit(handle)=handle.as_raw() {
        unsafe {
            let native:id=msg_send![handle.ns_view.as_ptr() as id,window];
            let frame:NSRect=msg_send![native,frame];
            Some(Rect{x:frame.origin.x,y:primary_height()-frame.origin.y-frame.size.height,w:frame.size.width,h:frame.size.height})
        }
    } else {None}
}
