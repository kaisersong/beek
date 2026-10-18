pub mod core;
pub mod platform;
#[cfg(any(windows, target_os = "macos"))]
#[rustfmt::skip]
pub mod icons;
