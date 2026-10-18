#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
fn main() {
    beek::platform::windows::helper::run();
}
#[cfg(not(windows))]
fn main() {
    eprintln!("The selection host is Windows-only.");
}
