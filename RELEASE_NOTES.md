## BeeK v1.1.5

### macOS 浏览器授权与菜单整理

- “开启自动化授权（控制浏览器）…”先选择已运行的浏览器，再主动请求系统授权；首次安装不再只打开没有 BeeK 条目的自动化设置页。
- 已拒绝过的授权会引导用户到系统设置恢复；补充 Apple Events 授权用途说明。
- 移除菜单栏、应用菜单和 Dock 菜单中重复的“豆包 API Key…”入口，统一在翻译服务中配置 Key 和模型。
- 首次授权、拒绝恢复仍需在签名安装包上完成真实交互验证。

### Windows 更新检查修正

- 更新检查改用 GitHub 公开的最新版发布入口，移除对匿名 REST API 的依赖，避免该 API 配额用尽时出现 HTTP 403。
- 发现新版本时，仍要求该版本的 Windows x64 安装器与便携包齐全；不会推荐预发布版、其他仓库的包或降级版本。下载和安装保持手动。
- 修复已在 yzjw 本地安装构建中通过托盘菜单实测，原配置保留。原始 403 未在排查时重现，限流仅为疑似原因。

### 蜜蜂按钮自动隐藏

- macOS 和 Windows 的蜜蜂按钮在显示 5 秒、用户未点击时自动隐藏。
- 重新划词会重新计时；已点击并进入翻译的按钮交由翻译流程处理。
- 隐藏按钮保留已有译文卡片；macOS 不会因为同一旧选区的 AX 恢复而重新弹出按钮。

### macOS 云之家取词修正

- 修复云之家辅助功能接口返回选区漏字、错位时，BeeK 使用错误片段解释的问题。
- 云之家改用应用自身“拷贝”命令获取完整文本，完整还原剪贴板的文本、图片及其他类型，并标记自身写入，避免复制监听重复触发。
- 每次新划词或键盘改选按状态去重；普通点击、来源切换或同一选区轮询不会反复执行拷贝。拷贝失败时不退回已知错误的 AX 文本。
- 本次兼容处理针对云之家，不代表其他 Electron 应用均存在同一问题。

### 技术与验证

- BeeK 使用 Rust + GPUI 开发；中英文 README 补充技术说明、按钮超时行为和云之家取词路径。
- 修复代码在 macOS 标准格式检查、90 项 locked Rust 测试及 release 构建中通过；双机本地安装通过原生窗口和按钮超时自检，云之家真实划词由用户确认标题完整。
- 合成文本的原生 Copy 成功路径、PNG / 纯文本 / 自定义剪贴板类型还原及 own-write 标记通过。本轮原生内存压力查询仍缺少诊断进程的辅助功能授权，没有新的压力通过证据。
- Windows 更新修复通过格式检查、73 项 locked Rust 测试及 27 项 Windows 工具测试；两台 Windows 机器的显式在线检查通过。macOS 开发 shader 配置下 92 项 Rust 测试通过。
- v1.1.5 的双平台安装包、macOS 签名公证和资产校验结果以本次 Release 工作流为准。

### English

Windows update checks now use GitHub's public latest-release page instead of the anonymous REST API. Checks still require both exact Windows x64 packages and reject prerelease tags or downgrades. The fix was verified through the tray menu on yzjw with existing settings preserved. The originally reported HTTP 403 was not reproduced during investigation; API rate limiting remains a suspected cause.

Unclicked bee buttons now hide after five seconds on macOS and Windows. New selections get a fresh timer, and existing translation cards remain open.

On macOS, CloudHub uses its own Copy command to obtain the complete selection when Accessibility returns shifted text. All clipboard types are restored, including images, and repeated polling does not repeatedly copy the same selection. This workaround currently applies to CloudHub.

BeeK is built with Rust + GPUI. Both READMEs now describe the stack and updated selection behavior. Final package availability, signing, notarization and asset validation are determined by the v1.1.5 Release workflow.
