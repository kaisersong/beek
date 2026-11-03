## BeeK v1.1.1

### macOS

- 修复更新提示反复切换 Dock 显示状态的问题；更新窗口现在保留用户的 Dock 显示设置。

- 全新四步首次使用引导，介绍取词权限、点击翻译与服务配置，重点推荐豆包；试译按钮直接可见，无需选中示例。
- 中英文界面，可选跟随系统；独立选择8种翻译目标语言：简体中文、英文、日文、韩文、繁体中文、法文、德文、西班牙文。
- 可切换免费公共百度、自己的百度 Key、豆包 / OpenAI 兼容、Azure、Google、MyMemory 和 LibreTranslate / Argos。公共百度无需用户填写 Key，共享额度有限；其他服务使用用户自己的账号与额度。
- 所有引擎使用近期结果缓存，相同内容命中后直接展示。默认128条、30分钟，可关闭；最多1000条、24小时、总计2MiB，退出应用后清空。缓存按引擎、账号、请求设置和目标语言隔离。
- 内置中英文本地翻译安装说明及可复制的 AI 安装提示词；BeeK 不会自动执行安装命令。
- 修复译文完成后卡片消失、浮动按钮与多屏定位问题。
- 修复检查更新时菜单变灰：立即展示进度，结果使用非模态窗口，重复点击合并处理；修复语言子菜单被自动禁用。
- 包含主分支已有的更新安装改进：先准备完整 bundle，退出旧进程后切换，保留签名，并在启动失败时回退到 LaunchServices。

### Windows 11 x64 公开预览

- 修复 Chrome 首次取词的焦点事件使选区失效，以及内部子窗口动画关闭翻译卡片的问题。
- 改进桌面测试工具的焦点处理及网络错误夹具。
- 本次 Windows 仍使用原有界面与豆包翻译设置；macOS 的新引导、中英文界面、目标语言菜单、多引擎和缓存设置尚未同步到 Windows。
- Windows 包为未签名公开预览，可能显示未知发布者提示，尚未完成完整多屏 / 混合 DPI 验收。

### 验证与安装

- macOS 本地执行 locked Rust 测试、更新安装脚本测试及 Python 工具测试；原生菜单48组语言组合、16项更新窗口状态检查通过。真实界面工具连接失败，菜单鼠标点击仍需实际使用确认，未宣称全应用或跨天验收。
- Windows 已在独立桌面验证 Chrome、Edge、Notepad 的选区及译文持续展示；本次构建另由 CI 执行源码测试与打包，不代表所有桌面场景验收完成。
- macOS 分发包由 Release 工作流签名、公证并执行 Gatekeeper 验证；工作流结果为准。
- macOS 下载 `BeeK.dmg`，打开后拖入“应用程序”；自动更新使用 `BeeK-v1.1.1.zip`。
- Windows 下载 `BeeK-windows-x64-setup.exe`，或完整解压便携 ZIP 后运行 `beek.exe`，保留配套 helper。Windows 后续更新从 Releases 手动下载。
- 提供 SHA256 校验文件。

### English

BeeK 1.1.1 includes the 1.1 feature set: guided setup, Chinese/English UI, eight translation targets, switchable providers, a shared free Baidu option, and a bounded cache across connected engines on macOS. It also fixes disappearing translation cards, update dialogs blocking menus, and disabled language submenus. Windows remains an unsigned preview with selection reliability fixes; the new macOS settings and onboarding are not yet available on Windows. Local services require installing the appropriate language models. Shared service quotas are limited; personal providers use your own account quota.
