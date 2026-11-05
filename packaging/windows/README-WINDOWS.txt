BeeK for Windows 11 x64

Run beek.exe together with the matching beek-selection-host.exe. The pair is
verified at startup; do not mix files from different builds. Rust and Visual
Studio are not needed at runtime. The MSVC CRT is statically linked.

Select text in a supported application, then click the bee to translate. No text
is sent before your click. Ctrl+Alt+T opens a local snapshot; press Enter to send.
Copy monitoring and login startup are off by default. The tray's Read clipboard
command opens a local preview, without simulating Ctrl+C. Updates are checked after 10 seconds and every 6 hours (configurable);
Windows never automatically downloads or installs an update.

On first launch, follow the welcome page and choose a translation service in
Settings. New users start with free public Baidu translation (no API key).
Existing Ark settings and saved keys are preserved. Seven services are available:
Baidu public, Doubao / OpenAI-compatible, Azure, Baidu with your own key,
Google Cloud Translation, MyMemory, and a running LibreTranslate / Argos server.
Each service keeps its own settings and Windows Credential Manager key.

Choose one of eight target languages and Chinese / English / system UI language.
Click Save (Ctrl+S) to apply. Try translation sends a public example only when
clicked. The tray's Getting started command reopens the welcome page.

Results are cached in memory: 128 entries, 30 minutes by default, at most 2 MiB.
Change the entry count (0-1000) and lifetime (0-1440 minutes) in Settings.
Set either value to 0 to disable caching. Baidu public and personal-key services
are excluded from caching. Errors and interrupted streams are never cached.
Selecting another passage by dragging keeps the current card; click its bee to
replace the card, or click outside / the card's close button to dismiss it.

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
