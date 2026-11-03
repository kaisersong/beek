# <img src="resources/icons/BeeK-app-256.png" width="40" height="40" alt="BeeK icon"> BeeK

Select text. Click the bee. Understand more.

A lightweight selection translator for macOS and Windows 11 x64.

[简体中文](README.md)

[![Latest release](https://img.shields.io/github/v/release/kaisersong/beek)](https://github.com/kaisersong/beek/releases/latest)

**[Download for macOS](https://github.com/kaisersong/beek/releases/latest/download/BeeK.dmg)** · **[Windows installer](https://github.com/kaisersong/beek/releases/latest/download/BeeK-windows-x64-setup.exe)** · [Windows portable ZIP](https://github.com/kaisersong/beek/releases/latest/download/BeeK-windows-x64.zip) · [All releases](https://github.com/kaisersong/beek/releases)

<p align="center">
  <img src="resources/screenshots/onboarding-welcome-en.png" width="960" alt="BeeK macOS welcome screen, with a translation card preview and a Get started button.">
</p>

**1.1.1 is available**: four-step guided setup, Chinese/English UI, eight translation targets, switchable services, and a shared result cache on macOS. It also fixes update dialogs blocking menus, disabled language submenus, and update notices changing Dock visibility. [Release notes](https://github.com/kaisersong/beek/releases/tag/v1.1.1)

| Feature | macOS | Windows 11 x64 |
| --- | --- | --- |
| Select text → click the bee → translate | Available | Available |
| Doubao translation and word explanations | Available | Available |
| New onboarding, bilingual UI, target-language picker | Available | Not yet available |
| Multiple providers and configurable result cache | Available | Not yet available |
| Distribution | Developer ID signed and notarized | Unsigned public preview |

## Install

**macOS:** download `BeeK.dmg`, open it, and drag BeeK into Applications. Start BeeK and enable Accessibility when prompted. The released macOS app is signed and notarized. The versioned ZIP is used by the built-in updater.

**Windows:** run the installer and open BeeK from the Start menu. For the portable version, extract the whole ZIP and keep `beek.exe` together with its matching `beek-selection-host.exe`. Windows may show an unknown-publisher or SmartScreen warning. SHA256 checksums are included in the release. Full mixed-DPI/multiple-monitor acceptance is still pending. See the [Windows usage guide](packaging/windows/README-WINDOWS.txt).

## Start translating on macOS

The four-step onboarding covers welcome, selection gestures, service setup, and permissions with a trial translation. You can reopen it from **Getting started…** in the BeeK menu.

1. Enable BeeK under **System Settings → Privacy & Security → Accessibility**.
2. Choose a translation service. New macOS users can start with the shared Baidu service without a key.
3. Click **Try translation** to test the public sample. You do not need to select the sample text.
4. Select text in another app, then click the floating bee. A selection alone does not send a translation request.

**Doubao is recommended** for streaming translation and dictionary explanations. Create an API key in the [Volcengine Ark console](https://console.volcengine.com/ark/), enable your model, and enter the key and optional model/endpoint ID through onboarding or **Doubao API key…**. Calls use your own Ark account quota. Existing configurations keep their selected service and credentials.

For browser selection, BeeK may also request Automation permission. If you previously denied it, use **System Settings → Privacy & Security → Automation**. Other selection fallbacks remain available.

## Translation services on macOS

Open **Translation service: …** to switch between shared Baidu, your own Baidu key, Doubao / OpenAI-compatible APIs, Azure Translator, Google Cloud Translation, MyMemory, or LibreTranslate / Argos. Each service keeps its own settings.

The shared Baidu service requires no key in the app, has limited shared quota, and accepts up to 1,500 UTF-8 bytes per request. Text is forwarded through the BeeK gateway to Baidu. Other hosted services use your own credentials and account quota. Dictionary explanations are provided by Doubao / OpenAI-compatible models; the other services return plain translations.

## Two independent language settings

- **App language:** system default, Simplified Chinese, or English. This changes the interface.
- **Translate to:** Simplified Chinese, English, Japanese, Korean, Traditional Chinese, French, German, or Spanish. This changes the translation request; the default is Simplified Chinese.

Both pickers are in the menu-bar menu and onboarding. Local services depend on installed language models; MyMemory requires an explicit source language.

## Result cache on macOS

All connected engines use a bounded, process-local cache. Successful complete results can be displayed immediately without another model or service request. Different providers, accounts, request settings, and target languages have separate cache identities.

Open **Translation cache…** to change the limits. Defaults are **128 entries / 30 minutes**; maximums are **1,000 entries / 24 hours / 2 MiB total**. Set either entry count or lifetime to zero to disable it. Cached results are cleared when BeeK exits.

## Local translation

LibreTranslate provides the API and Argos provides local models. BeeK includes an offline guide and a copyable AI setup prompt under **Translation service: … → LibreTranslate / Argos → Local installation guide…**. Copying instructions does not execute commands or install models.

- [Installation guide](resources/help/libretranslate-install.en.txt)
- [AI setup prompt](resources/help/libretranslate-agent-prompt.en.txt)

Use a loopback endpoint such as `http://127.0.0.1:5000/translate`. Install and verify the required source/target model pair with a public sample before enabling the service in BeeK.

## Updates

On macOS, **Check for updates…** immediately shows progress in a modeless window. Downloading and installing are separate choices, menus remain usable, and update notices preserve your Dock setting. Automatic checks can prepare updates in the background for installation on exit.

Windows does not automatically download or install updates; download a newer package from [Releases](https://github.com/kaisersong/beek/releases).

See the [Chinese README](README.md) for source builds, configuration examples, selection internals, and development tools. Internal engineering documents are maintained in the separate mydocs repository; they are not required to build BeeK.
