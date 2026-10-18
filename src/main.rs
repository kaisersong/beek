#![cfg_attr(windows, windows_subsystem = "windows")]

// Keep the existing macOS entry point at the crate root: its module paths and
// native behavior remain identical while Windows has an independent boundary.
#[cfg(target_os = "macos")]
include!("macos_main.rs");

#[cfg(windows)]
fn main() {
    beek::platform::windows::run();
}

#[cfg(not(any(windows, target_os = "macos")))]
fn main() {
    eprintln!("BeeK supports macOS and Windows 11 x64.");
}
