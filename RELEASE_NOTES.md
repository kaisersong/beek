## BeeK v1.1.2

### Windows 11 x64 公开预览

- 四步使用引导对齐 macOS 的云海背景、版式与交互：欢迎、划词演示、翻译服务、试译完成；可从设置或托盘重开。
- 支持跟随系统 / 简体中文 / English 界面，以及独立选择 8 种翻译目标语言。
- 支持公共百度、豆包 / OpenAI 兼容、Azure、个人百度 Key、Google、MyMemory、LibreTranslate / Argos。新用户默认公共百度免 Key；旧用户保留既有引擎、配置与凭据，各服务密钥独立存入 Windows Credential Manager。
- 新增有界进程内缓存及设置，默认 128 条 / 30 分钟，可关闭；公共 / 个人百度模式不使用缓存。缓存不写磁盘，按服务、账号、目标语言及请求参数隔离。
- 引导中的服务保存返回试译页，只有主动点击试译才发送公开例句；演示在本地完成。保存服务不会误标引导完成。
- 拖选新文字时保留当前翻译卡片，点击新蜜蜂后才替换；普通外部点击和关闭按钮可收起卡片。
- 可选自动检查更新，安装仍由用户手动完成。修复切换服务后的输入焦点与 Ctrl+S 保存。
- 改进便携 ZIP 打包，按 manifest 核对文件清单、大小、SHA256 和 ZIP CRC。

### 两平台读音与复制修正

- 只有真实 AI 词典结果才拆分完整读音首行；普通译文中含“读音：”也完整保留。
- 无读音时不显示占位，部分流式首行保持原文。
- 加载、错误和纯空白结果不覆盖剪贴板、不显示复制成功反馈；仍可复制当前已收到的流式正文。
- 词典复制包含原词和可用读音，其他服务保持普通译文。朗读对象仍是原词。
- 本版继续使用原有翻译提示词；简短释义 / 短语提示词实验未纳入本版。

### 验证与安装

- 本地 macOS locked Rust 测试 88 项通过（使用已有 runtime shader feature），Windows 交互测试 71 项通过；Windows 工具 27 项、离线内容门槛夹具 17 项通过。
- macOS 真实剪贴板入口 9 项、Windows Panel 复制入口 10 项通过。Windows 96 DPI 与 144 DPI 产品窗口检查通过；完整多屏 / 混合 DPI、200% 缩放与跨天验收仍未完成。macOS AX 内存压测无新的通过证据。
- macOS 分发包由本次 Release 工作流签名、公证并执行 Gatekeeper 验证，结果以工作流为准。Windows 为未签名公开预览，可能显示未知发布者提示。
- macOS 下载 `BeeK.dmg`，打开后拖入“应用程序”；内置更新器使用 `BeeK-v1.1.2.zip`。
- Windows 下载 `BeeK-windows-x64-setup.exe`，或完整解压便携 ZIP，保留同目录的 `beek-selection-host.exe`。提供 SHA256 校验文件。

### English

Windows 1.1.2 gains the macOS-style four-step onboarding, bilingual UI, eight translation targets, seven providers and configurable process-local caching. Existing credentials and service settings are preserved. New selections keep the current translation card until the next bee click. Update checks are optional; installation remains manual.

Both platforms preserve ordinary translations that start with a pronunciation prefix, omit missing-pronunciation placeholders, and prevent loading/error/blank results from overwriting the clipboard. Partial streamed results remain copyable. Translation prompts remain unchanged.

macOS distribution uses the release signing/notarization pipeline. Windows is an unsigned public preview; full mixed-DPI, multiple-monitor and long-running acceptance is still pending. Build and artifact availability are determined by this release workflow.
