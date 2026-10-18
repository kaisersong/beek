# Final m6 call-site evidence

The purge branch ALREADY dispatches directly at the Windows entry point, before ui::run, platform checks, instance setup, and install_guard. It exits immediately after cleanup. The previous snapshot simply omitted this dependency. Do not unlock the uninstaller early. Please close m6 after checking the actual call sites; the other six issues were CLOSED in R2. G5 remains NOT PASSED.

```rust
mod diagnostics;
pub mod helper;
mod input;
mod integration;
mod lifecycle;
mod native;
mod pair;
mod process;
mod selection;
mod speech;
mod store;
mod text_model;
mod ui;
mod util;

pub fn run() {
    if std::env::args().nth(1).as_deref() == Some("--purge-user-data") {
        if store::Store::purge_user_data().is_err() {
            std::process::exit(1);
        }
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--verify-pair") {
        match pair::helper() {
            Ok(_) => println!(
                "{{\"paired\":true,\"build_id\":\"{}\"}}",
                env!("BEEK_BUILD_ID")
            ),
            Err(_) => {
                println!("{{\"paired\":false}}");
                std::process::exit(3);
            }
        }
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("--capabilities") {
        println!("{}", capabilities("main"));
        return;
    }
    let mut session = 0;
    if unsafe {
        windows::Win32::System::RemoteDesktop::ProcessIdToSessionId(
            windows::Win32::System::Threading::GetCurrentProcessId(),
            &mut session,
        )
    }
    .is_err()
        || session == 0
    {
        eprintln!("BeeK requires a logged-in interactive Windows desktop.");
        std::process::exit(2);
    }
    diagnostics::initialize();
    diagnostics::event("started", capabilities("main"));
    if let Err(error) = ui::run() {
        util::fatal(&error);
    }
}
pub fn capabilities(binary: &str) -> serde_json::Value {
    serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"build_id":env!("BEEK_BUILD_ID"),"rustc":env!("BEEK_RUSTC_VERSION"),"protocol":crate::core::protocol::VERSION,"binary":binary,"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"native_machine":util::native_machine(),"debug_assertions":cfg!(debug_assertions),"test_fixtures":cfg!(feature="test-fixtures")})
}

#[cfg(feature = "test-fixtures")]
pub mod harness;

```

Start of ui::run:
```rust
pub fn run() -> Result<(), String> {
    util::check_platform()?;
    let _installation = super::lifecycle::install_guard()?;

```
The named _installation File is held on the synchronous ui::run stack until Application::run returns. Purge does not invoke ui::run.
