BeeK for Windows 11 x64

Run beek.exe together with the matching beek-selection-host.exe. The pair is
verified at startup; do not mix files from different builds. Rust and Visual
Studio are not needed at runtime. The MSVC CRT is statically linked.

Select text in a supported application, then click the bee to translate. No text
is sent before your click. Ctrl+Alt+T opens a local snapshot; press Enter to send.
Copy monitoring and login startup are off by default. The tray's Read clipboard
command opens a local preview, without simulating Ctrl+C. The first Windows
version never automatically downloads or installs an update.

Settings: %APPDATA%\BeeK\config.json
Keys: current Windows user's Credential Manager (immutable BeeK/ark references).
Logs: %LOCALAPPDATA%\BeeK\logs (metadata only, bounded; no text or API key).

Close visible windows to leave BeeK in the tray. Use Quit BeeK to exit.
Portable files can be removed after exiting; user data remains. The per-user
installer supports uninstall while preserving data by default. Interactive
uninstall offers a separate explicit permanent-data-removal choice.

Windows downloads are currently unsigned public previews (manifest distribution:
public-preview-unsigned). Windows may show an unknown-publisher / SmartScreen
prompt. Verify the published SHA256 checksum before installing. Complete mixed
DPI/multiple-monitor acceptance and signed distribution remain pending; public
preview availability does not mean those acceptance gates have passed.
Internal-preview-unsigned packages are for development only.
