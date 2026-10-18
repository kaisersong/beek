# BeeK Windows G0 probe

Independent Rust 1.99.1 / GPUI 0.2.2 / windows 0.61.3 experiment. It intentionally does not import the macOS BeeK binary or claim to implement the Windows product.

On a Windows 11 x64 interactive desktop with MSVC C++ Build Tools, Windows SDK (x64 FXC), and Python 3.9+:

```powershell
powershell -NoProfile -File scripts/windows/run-g0.ps1
```

Run that command from the repository root. It performs locked debug/release builds and tests, gathers environment metadata, starts the exact binary whose digest is recorded, and leaves manual checks NOT RUN until evidence is supplied. Rustup reads the pinned toolchain when run from this crate. The Windows release renderer requires FXC; the script locates it in the SDK.

Direct build:

```powershell
cd tools/windows-probe
cargo build --locked --release
cargo run --locked --release -- probe.jsonl
```

The JSONL argument is optional; it contains only structural events, handles, timing, size/DPI and click counts. No keyboard history, input text, clipboard content or API key is logged. The probe never requests translation or simulates copy.

The input window has two GPUI fields with Windows edit shortcuts, Tab/Shift+Tab and real EntityInputHandler IME support. The button schedules a popup after 3 seconds so you can return to Notepad first. The popup is created hidden, adapted through raw-window-handle, given WS_EX_NOACTIVATE/TOOLWINDOW and a WM_MOUSEACTIVATE subclass, then shown without activation. Its native behavior is a hypothesis until G0 evidence passes.

A dedicated native message thread owns the notification icon and hidden top-level window, handles TaskbarCreated, and sends bounded commands. Use the tray menu "Close all probe windows" and wait 35 seconds to test residence; reopen using the tray. The tray's right menu can restore input, schedule the popup or exit.

Full procedure, all 18 G0 cases and the complete G0–G5 test plan: [Windows test plan](../../docs/windows/windows-test-plan.md). Do not enter personal text or credentials into this test UI. GPUI input adaptation attribution: [third-party notices](THIRD_PARTY_NOTICES.md).

生命周期适配说明：GPUI0.2.2 Windows在最后一个框架窗口销毁时会PostQuitMessage。原型使用公开API建立一个永久1×1且show=false/focus=false的生命周期宿主；它不显示、不进任务栏，明确只在退出时释放。日志分开记录visible_windows和gpui_windows，不能用隐藏宿主冒充零个框架窗口。
