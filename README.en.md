# BeeK

A lightweight selection translator for macOS and Windows.

**BeeK 1.1.0 for macOS** introduces a new guided setup, Chinese/English app language, and selectable translation targets. Windows receives selection reliability fixes in this version; its settings and translation flow remain separate.

Select text in another app, click the floating bee, and read its translation. BeeK can stream results with Doubao and show dictionary explanations for individual words.

## Getting started on macOS

Open **Getting started…** from the BeeK menu. Enable Accessibility when prompted, choose a translation service, then click **Try translation**. You do not need to select the sample text.

Doubao is recommended: create an API key in the [Volcengine Ark console](https://console.volcengine.com/ark/), enable a model, then enter the key and optional model/endpoint ID in BeeK. Calls use your own Ark account quota. You may also start with the free shared Baidu service without a key, or configure your own supported service.

## Two independent language settings

- **App language**: system default, Simplified Chinese, or English. This changes the interface.
- **Translate to**: Simplified Chinese, English, Japanese, Korean, Traditional Chinese, French, German, or Spanish. This changes translation requests.

Both settings are available in the menu-bar menu; guided setup also provides language pickers. Existing configurations keep their service and credentials. The default translation target remains Simplified Chinese.

All connected macOS engines use the selected target language and a bounded in-memory result cache. Results for different target languages stay separate; switching back can reuse an unexpired result. Configure the entry limit and lifetime in **Translation cache…**. The cache is cleared on exit.

## Local translation

LibreTranslate provides the API and Argos provides local models. BeeK includes an offline English setup guide and an AI setup prompt under **Translation service… → LibreTranslate / Argos → Local installation guide…**. Copying instructions does not execute commands.

Use a loopback endpoint such as `http://127.0.0.1:5000/translate`. Install and verify the actual source/target model pair before selecting a target in BeeK. MyMemory requires an explicit source language; local services depend on their installed models.

See the [Chinese README](README.md) for the full development and packaging workflow. The new onboarding and language controls described above are macOS features; Windows runtime/UI verification is separate.
