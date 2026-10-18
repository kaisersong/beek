# BeeK Windows 版详细实施方案

- 文档版本：v1，待三方对抗评审。
- 日期：2026-10-17。
- 源码基线：`09cfe6d3df02ddbd5bc274e1590d6f8ec757bad6`，Cargo 版本 `1.0.14`。
- 本轮交付：实施方案、独立评审、修订与共识记录。不实施业务代码、不发布、不修改用户配置。
- 本方案使用能力、依赖、验收证据安排 AI 开发，不估算人天。
- “设计评审通过”只表示可按此执行；不能替代 Windows 构建、真实应用兼容性和发布验收。

## 1. 目标、范围与默认行为

保留 Rust 和 GPUI，将现有 macOS 专用代码隔离，实现 Windows 上“选中文本 → 小蜜蜂 → 点击才请求翻译 → 流式弹窗”的完整主路径。

首个公开 Windows 版本支持 Windows 11 x64、普通用户权限、本地交互桌面。Windows 10、ARM64、远程桌面、管理员程序、游戏和独占全屏不作首版支持承诺；它们的失败必须安全降级，不能导致常驻进程崩溃或复制错误内容。不得将部分软件通过误写成“全局取词适用于任何应用”。

| 能力 | 首版行为 | 验收边界 |
| --- | --- | --- |
| 自动划词 | 默认开启；拖选、双击后查 UIA 选区 | 支持清单中 UIA 可用的软件必须直接取词 |
| 点击才翻译 | 默认且不可被后台探测绕过 | 展示小蜜蜂不产生模型请求 |
| 词典/段落翻译、流式显示 | 复用当前逻辑 | 网络失败可重试；旧响应不能复活已关闭的弹窗 |
| 复制译文、拖动、点外关闭 | 保留 | 不破坏原应用选区，不误关闭当前交互 |
| 用户复制后显示小蜜蜂 | 可选，默认关闭 | 仅开关打开时处理外部文本剪贴板变化 |
| 全局快捷键触发 | 默认 `Ctrl+Alt+T`；可改、可关闭 | 注册冲突有反馈，失败不劫持按键；仅查询当前选区 |
| 自动模拟 Ctrl+C | 首版默认关闭；独立实验能力门禁 | 不作为 UIA 失败后无提示的必经路径 |
| 托盘、设置、关于、退出、重启 | 必须 | 无可见窗口仍常驻；资源可释放 |
| 登录启动 | 独立开关，默认关闭 | 退出不自动改变登录启动偏好 |
| 系统朗读 | 可选，有语音则可用 | 无对应语音时给明确反馈，不阻塞翻译 |
| 更新 | 首版检查并引导用户下载签名安装包 | 自动替换更新另设门禁，未通过时不得自动下载执行 |

自动复制和自动安装均有明确独立的后续设计，不能由 AI 开发时自行打开。首版对 UIA 不支持的软件提供“开启复制即显示，手动 Ctrl+C”的降级路径，不要求安装浏览器扩展，不使用 OCR、注入 DLL、浏览器调试端口或管理员权限兜底。

## 2. 本地代码证据

仓库没有 `.codegraph/`，按用户规则跳过索引，不自行初始化。

| 文件 | 基线中的职责和移植影响 |
| --- | --- |
| `Cargo.toml:13` | GPUI 0.2.2；macOS crate 目前全局引入；Ely 为未固定提交的 Git 依赖 |
| `.gitignore:2` | 忽略 Cargo.lock，干净构建不可复现 |
| `src/translate.rs` | 299 行，HTTP/SSE、词典提示词和响应解析，可保持行为复用 |
| `src/config.rs:66` | 使用 HOME 和 Library/Application Support，Windows 必须分平台选目录 |
| `src/ax.rs:489` | AX 选区查找；1209 行还包含探针与菜单搜索，不能机械逐行翻译 |
| `src/appcopy.rs:136` | AppleScript 浏览器取词，以及 AXPress 复制菜单；Windows 无直接等价路径 |
| `src/clipboard.rs:150` | NSPasteboard 多类型备份；Windows 数据格式和所有权机制不同 |
| `src/mouse.rs` | Cocoa 鼠标坐标、主屏高度、按键状态 |
| `src/eventtap.rs` | CGEventTap；当前主要为诊断取证使用，正常运行仍有鼠标轮询 |
| `src/main.rs:461` | 仅按主屏尺寸布局；`PopupGeom::card_cocoa` 还有坐标翻转 |
| `src/main.rs:491`、`:766` | GPUI 透明 PopUp，`focus:false`；Windows 必须验证原生实际行为 |
| `src/main.rs:1108` | 50ms AX 轮询和 macOS 权限判断混入应用状态机 |
| `src/main.rs:1728`、`:1884` | Dock、AppKit 菜单和启动初始化混入主程序 |
| `src/menubar.rs:156`、`:284` | NSWindow 移动；重启仅 exit(0)，依赖 launchd 重新拉起 |
| `src/dialogs.rs` | 原生 NSAlert 与输入框，需要隔离或改 GPUI 设置窗口 |
| `src/update.rs:70`、`:255` | 通用资产名、ditto、.app、Shell 替换流程，不可给 Windows 复用 |
| `.github/workflows/release.yml:38` | 仅 macos-14 的签名、公证、DMG 发布路径 |

Ely 当前源码未被 BeeK 引用。第一步验证移除它后现有行为不变，并提交应用 Cargo.lock；若后续确需组件库，单独立项验证兼容并固定版本，不能借移植隐式升级 GPUI 全栈。

## 3. 技术选择与可证伪条件

1. 优先保留 GPUI 0.2.2。上游有 Windows 后端，但不意味着 `focus:false`、透明 PopUp、置顶和托盘存活均满足 BeeK。必须先完成 G0 原型门禁。
2. Windows API 通过 `windows` crate 访问；具体版本由最小构建与 GPUI 依赖兼容检查确定后锁定。原生窗口句柄经 raw-window-handle 公开接口取得，不通过按窗口宽度枚举猜测。
3. 托盘首选 Win32 `Shell_NotifyIconW`，与消息窗口一起放在专属平台消息线程；不为几个菜单额外引入不受控 UI 运行时。
4. 设置、关于和错误信息使用 GPUI 窗口；仅系统必要界面使用 Win32。设置窗口允许正常激活，悬浮按钮和翻译窗口采取非激活策略。
5. UIA 放入独立 `beek-selection-host.exe` 进程，通过有界 JSON Lines 通信；进程内 COM MTA 线程执行 UIA，不让跨进程 UIA 卡住主 UI 或输入钩子。
6. 取词、复制监视和窗口事件都归入单一应用状态机，通过 source token 和 generation 丢弃过期消息。
7. 首版不实现浏览器扩展。只有验收证明 UIA 与显式复制路径都不足，才另写扩展方案和权限说明。

G0 失败时的顺序：公开 API 调整 → Windows 专用窗口样式适配 → 可审计的最小 GPUI 补丁并固定修订。若仍不能满足非激活交互，则停止依赖该假设的开发、修订此方案并重审；不自动改成 Electron/Tauri，不擅自替换整个 UI 栈。

## 4. 目标模块及边界

```text
src/
  main.rs                       # 仅引导与依赖装配
  app/{controller,events}.rs     # 选区、请求、弹窗、退出状态机
  core/{selection,geometry}.rs   # 平台中立值对象与布局规则
  ui/{icon,popup,settings,about}.rs
  translate.rs                  # 既有翻译能力与可替换测试端点
  config.rs                     # schema、迁移与原子写入
  platform/
    mod.rs                      # 接口、能力、错误
    macos/...                   # 既有 AX/AppKit/剪贴板逻辑
    windows/
      input.rs                  # 钩子/热键，只发送事件
      desktop.rs                # HWND/PID/前台对象、显示器与 DPI
      overlay.rs                # 非激活窗口样式和位置
      tray.rs                   # 通知区图标和菜单
      clipboard.rs              # 显式复制监视；可选事务
      lifecycle.rs              # 单实例、自启动、显式重启
      speech.rs                 # 系统语音
  selection_host/...            # 独立 Windows UIA helper
  update/{check,manifest,windows,macos}.rs
```

允许按真实代码调整文件粒度，但以下边界不可取消：core 不导入 AX/UIA/objc；ui 不查全局选区；输入回调不等待网络/UIA；平台代码不直接发翻译请求；Windows 的 cfg 分支不引用 macOS crate；macOS 模块保留原平台默认行为。

接口草案：

```rust
struct SourceToken { hwnd: u64, pid: u32, generation: u64 }
struct Selection { text: String, rects: Vec<PhysicalRect>, source: SourceToken }
enum SelectionResult { Selected(Selection), Empty, Unsupported, Protected, TimedOut, Stale }
enum Trigger { DragEnded, DoubleClick, KeyboardSelection, Hotkey, UserCopy }
enum PlatformEvent { Trigger(Trigger, SourceToken), OutsidePress, DisplayChanged, SessionChanged }
```

这是语义草案而非可直接编译接口。真实实现需封装句柄生命周期和并发；UIA COM 对象不跨进程传递，主进程只接收值数据。请求和剪贴板内容不进入托盘回调闭包。

## 5. 取词服务、触发与竞争控制

### 5.1 输入与源绑定

- 专用消息线程安装 `WH_MOUSE_LL`；需要键盘选区触发时安装 `WH_KEYBOARD_LL`，全局快捷键使用 `RegisterHotKey`。回调仅记录必要事件、时间和位置，立即返回；不记录用户键入内容。
- 拖选、双击、键盘选择结束后防抖 120ms，应用状态机持有一个最新 pending 请求；同一源不并发排队。数值为初始配置，需通过验收调优。
- 查询前记录前台 HWND、PID、进程创建标识、generation 和显示器。返回时复查来源和 generation；用户切换应用、开始新选区、打开 BeeK 设置或锁屏都使旧结果过期。
- 点击 BeeK 小蜜蜂时使用已缓存选区，不重新从 BeeK 自己查询。排除本进程、helper、密码框、受保护界面和设置输入框；不从无法证明安全的元素走复制兜底。
- 受支持控件可订阅 UIA TextSelectionChanged 作为增量触发；订阅与取消同一 MTA 线程完成，按前台源切换，不能全桌面无限订阅。

### 5.2 UIA 查询顺序

1. 验证前台源仍然一致且会话可交互。
2. 获取焦点元素，必要时按鼠标点 `ElementFromPoint`，在有界祖先范围查 `TextPattern`，避免整棵桌面树遍历。
3. 检查密码等受保护属性，读取 `GetSelection`，过滤空/退化范围，批量 `GetText` 和 `GetBoundingRectangles`。
4. 多段选区按文档顺序拼接，保留矩形；文本默认上限 16,384 Unicode 字符，超限提示缩小选区，不能悄悄截断送模型。
5. 无有效矩形时，鼠标触发使用触发点作为锚点；键盘触发仅在有可信控件/插入点位置时显示，否则提示快捷键失败，不在主屏任意位置弹窗。
6. 过期、空、受保护和不支持是不同返回类型。不得将控件 Value/整页文本冒充选区，也不得把无法取词等同“没有选中”。

### 5.3 helper 协议与故障隔离

- helper 与主进程来自同一安装包，用固定绝对路径启动，不搜索 PATH。stdin/stdout 仅供协议，stderr 为脱敏诊断。
- 握手包含协议版本、程序版本、实例随机 token；每条消息带 request_id、source、generation。单帧上限 256 KiB，拒绝未知版本、异常长度和无关响应。
- 主进程总超时初始 800ms；超时先丢弃该请求，再结束并重新创建 helper，不能只对一个已卡死线程设置逻辑超时。
- helper 纳入 Windows Job Object，主进程退出时结束子进程。连续三次故障采用 1/5/30 秒退避，达到阈值暂停自动取词并在托盘显示可恢复状态，避免无限重启。
- 锁屏、会话切换、休眠恢复时取消旧请求、清空缓存；解锁后重新建立源绑定。断管和协议错误等同可恢复 helper 故障。

## 6. 剪贴板及降级路径

### 6.1 首版显式用户复制

- `AddClipboardFormatListener` 在平台消息窗口监听；默认不开启业务处理。
- 用户打开“复制即显示”后，仅接受当前外部交互源的 Unicode 文本变化。自身复制译文及写回操作的 sequence 必须忽略。
- 限长、去空、URL 过滤复用 core 规则；显示按钮仍不请求翻译。不读取或上传图片、文件、非文本格式，不解析密码管理器等明确排除源。
- 外部复制无法总是证明与一次 Ctrl+C 一一对应；界面名称和说明明确“复制文本后显示”，不能声称精准识别用户快捷键。快捷键查询失败时说明可手动复制后触发。

### 6.2 后续实验性自动 Ctrl+C

Windows 不承诺任意格式的剪贴板无损快照。首版不以一个文本剪贴板库宣称完成 macOS 原实现的全类型恢复。

若实现此实验能力，必须同时满足：

1. 独立开关、默认关闭；说明会短暂改写系统剪贴板，可能被剪贴板历史/同步观察到，恢复不能撤回历史。
2. 仅白名单文本格式允许尝试：`CF_UNICODETEXT`、`CF_TEXT`、`CF_OEMTEXT`、`CF_LOCALE`。遇到图片、文件、私有格式、OLE/延迟渲染无法立即完整读取、锁竞争或不明 owner，返回 Unsupported，不清空、不发按键。
3. 快照失败与“原来为空”分开。持有有效消息 HWND 作为 owner，遵守 Open/CloseClipboard 与内存所有权规则；不在持锁时 SendInput。
4. 触发前再次验证源、前台、权限和修饰键状态；只对已确认的划词手势触发一次，不为普通单击发送 Ctrl+C。
5. 记录备份 sequence、模拟输入后的文本和 sequence；仅在确认仍为本次复制状态时尝试恢复。检测到第三方变化时不覆盖最新剪贴板。
6. Sequence 检查无法从理论上消除所有竞争；恢复失败要保留原备份并反馈，禁止承诺零副作用。用户可随时关闭此能力。
7. 普通权限不能注入更高完整性级别目标；识别后返回 Protected，不提升权限、不循环重试。
8. 必须单独通过剪贴板故障/并发测试后才可暴露开关；不通过不阻塞显式复制方案发布。

## 7. 悬浮窗、焦点、坐标与交互

- DPI awareness 在创建任何窗口前确定；目标 Per-Monitor V2，核对 GPUI 已有行为，不能在窗口创建后重复设置进程 DPI 模式。
- 系统选区和鼠标统一为虚拟桌面物理坐标，保留负坐标；`MonitorFromPoint/Rect` 选屏，以该屏工作区布局，并在平台边界转换 GPUI 所需坐标。禁止复用主屏高度翻转。
- 尺寸使用逻辑单位；UIA bounds、Win32 pixels、GPUI Pixels 的换算必须明确到显示器和 scale，不能仅全局乘一个 DPI 系数。
- 非激活窗口适配优先在创建/首次显示前设置 `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`，使用不激活的显示和移动方式；`WM_MOUSEACTIVATE` 策略须确保按钮仍能收到点击。
- 置顶只对 BeeK 弹窗使用，处理全屏、锁屏和虚拟桌面边界；不抢占 UAC/安全桌面。点击卡片复制、朗读可工作，输入设置窗口则正常激活。
- Win32 样式和 `SetWindowPos` 必须与 GPUI 实际 HWND、尺寸更新和生命周期一致；未通过 G0 不能把这些 API 名视作问题已解决。
- 在选区末端/触发点附近显示小蜜蜂，优先不遮挡选区；多行以触发端附近的矩形定位。跨屏按锚点所在屏布局，越界时约束到工作区。
- 外部按下关闭、按下小蜜蜂、卡片内按钮、拖动开始/结束在同一状态机排序；本轮按下不能立即关闭刚创建的弹窗。正在拖动或操作内部按钮时不得被取词检测撤销。
- 收到显示器移除、DPI 改变和工作区改变时重算位置；原显示器消失则移动到仍可用显示器，不能留下不可访问弹窗。
- 正常设置窗口支持键盘导航、中文 IME、粘贴、缩放和错误反馈；非激活卡片的键盘可访问替代路径由热键打开一个可激活结果窗口提供，关闭后恢复正常使用。

## 8. 配置、凭据与生命周期

- 用户配置放 `%APPDATA%\BeeK\config.json`；缓存和诊断放 `%LOCALAPPDATA%\BeeK\`；通过 Known Folders 获取路径，不拼接 HOME，不回退写工作目录。
- schema_version 版本化，新增字段显式平台默认值；修复 Rust derive Default 与 serde default 不一致的问题。旧 macOS 配置的行为保持兼容。
- 保存采用同目录临时文件、flush 和原子替换；失败保留旧文件并向设置窗口报告。损坏文件备份，不静默清空用户配置；配置修改串行化，防止读改写互相覆盖。
- API Key 首选 Windows Credential Manager；配置保留 credential_ref，环境变量 `BEEK_ARK_KEY` 仍为兜底。已有明文迁移时先确认凭据保存成功，再移除明文；失败保持可恢复。卸载默认保留用户数据，显式清理才删除配置和凭据。
- 保存模型设置时必须保留 `base_url` 等未编辑字段。日志不包含 API Key、完整请求正文、选区和译文；诊断仅 source 类型、错误码、耗时与尺寸，详细文本诊断需用户主动开启。
- 单实例使用当前用户/会话范围 mutex；第二次启动激活设置或托盘，helper 和 updater 子命令不抢主实例锁。IPC 仅限当前用户，验证请求类型。
- 自启动使用当前用户 Run 项，写入正确引用的绝对路径；安装迁移时更新，卸载时删除。不得把用户内容拼入命令行。普通退出保留自启动偏好，与 macOS 既有语义明确区分。
- Windows 重启显式拉起 current_exe 并完成单实例交接；禁止只 exit 并期待系统拉起。退出先注销热键/钩子/监听和托盘、取消请求、停止 helper，再退出；无窗口常驻不能依赖一个短暂弹窗续命。
- Explorer 重启后处理 `TaskbarCreated` 重新注册托盘；睡眠恢复和网络恢复可重建必要资源。
- 朗读使用 Windows 语音 API，专属正确 COM apartment 工作线程；不通过拼 PowerShell 字符串执行选区文本。没有语音或朗读失败不影响请求状态机。

## 9. 更新、打包和兼容发布

### 9.1 首版确定交付

- 新增 Windows x64 zip 和按用户安装的签名 installer，安装在用户程序目录，无需管理员权限。installer 工具在 G0 后选择一种并固定版本，不能同时维护两套安装框架。
- 包含主程序、selection helper、图标、许可证及实际依赖；检查 CRT、GPU/DirectX、资源路径和无控制台启动。程序资源定位相对 exe，不依赖 cwd。
- Windows 资产使用独立名称 `BeeK-v<V>-windows-x64.zip`、`BeeK-v<V>-windows-x64-setup.exe`；保留既有 `BeeK-v<V>.zip` 和 DMG，不能破坏老 macOS 更新器。
- Windows 更新检查必须匹配平台、架构与发布通道，找不到 Windows 资产应提示该版本无 Windows 包，不能下载 macOS zip 或误报更新成功。
- 首版仅打开固定仓库的 HTTPS 发布下载入口，由用户运行 installer；不静默下载、不执行在线任意 URL。签名缺失时只能产出内部预览包，不能标为正式发布通过。
- 版本、exe 版本资源、包名、helper 协议和关于窗口保持一致。安装/卸载正确处理托盘进程、快捷方式和自启动；更新保留配置、凭据和用户偏好。

### 9.2 后续自动更新门禁

- 使用独立 updater helper，主程序仅检查、下载、校验并暂存；updater 等待指定 PID 真正退出后替换，不能原地覆盖正在使用的 exe。
- 版本化 manifest 明确 platform/arch/version/package_type、大小和散列；散列只能检测损坏，真实性需要可信公钥签名元数据或固定发布者的 Authenticode 验证。不得将同站下载的 sha256 当作独立真实性证明。
- 校验 HTTPS 来源、重定向、大小上限和超时；解压拒绝绝对路径、`..`、链接及逃逸。主程序/helper/updater 都验证来源与版本，失败保留旧版。
- 安装过程采用 staging、备份、替换、成功标记、清理状态机；崩溃/断电/杀进程后能恢复，启动失败可回滚。不删除唯一可运行版本后再尝试复制。
- portable 与 installer 模式必须有标记并分别处理；便携只读目录不自动提升权限。首次自动更新协议与签名信任根作为单独评审对象。
- 没有签名基础设施、失败恢复或真实更新验收时继续使用手动更新，不以实现进度为由降低门禁。

## 10. 构建、测试及证据

### 10.1 开发环境与 CI

- 在真实 Windows x64 环境准备固定 Rust MSVC 工具链、Windows SDK 和实际构建所需编译组件；GPUI 的 shader/图形依赖按最小构建结果记录，不猜测必需工具。
- `Cargo.toml` 分 `cfg(target_os = "macos")` 和 `cfg(target_os = "windows")` 依赖。新增 helper 使用显式 bin，macOS 构建不得因 Windows helper 主入口缺失而失败。
- Windows CI 完成 locked 构建、核心测试、协议故障测试、安装包组装和资产校验；macOS CI 执行原平台构建与关键回归。缓存 key 包含 OS、工具链、Cargo.lock。
- 普通 PR CI 不持有发布密钥；签名发布仅受保护 tag/release 工作流。公共 GitHub runner 的非交互环境不作为 UIA、焦点和剪贴板真实桌面通过证据。
- 发布先分别构建平台资产，再由单一发布 job 汇总，避免 macOS/Windows 两个 job 同时修改同一 Release。保留现有 macOS tag 和资产兼容。

### 10.2 自动测试

| 测试类 | 必须覆盖 |
| --- | --- |
| core | 负坐标、多屏/混合 DPI、边缘布局、过期 generation、超限、空选区、重复点击 |
| 翻译 | 本地假 SSE 服务的分片、断流、错误、关闭后迟到响应；不使用真实 API Key |
| helper | 协议版本错、超长帧、乱序、断管、挂起、进程崩溃和重启退避 |
| 配置 | 新安装默认、旧字段兼容、损坏恢复、写失败、保留 base_url、并发更新不丢字段 |
| 生命周期 | 单实例、helper 清理、Explorer 重启、显式重启与登录启动独立 |
| 更新 | 资产选择无跨平台误取、坏签名/坏包、异常解压路径、替换失败与回滚（自动更新阶段） |

测试围绕契约和故障，不为静态文件结构和实现细节堆镜像测试。

### 10.3 交互桌面应用矩阵

| 环境/应用 | 需要记录的结果 |
| --- | --- |
| Windows 11 x64 + Edge/Chrome 普通网页 | UIA 自动划词、输入区、跨行、双击、焦点不丢失 |
| 记事本 | 拖选和键盘选区、中文/英文、空选区 |
| Word 或明确选定的一款办公文档应用 | UIA 支持情况、跨行与非文本内容 |
| VS Code/Electron 应用 | UIA 可用性；不支持时显式复制降级 |
| Windows Terminal | 原生复制快捷键差异、UIA/显式复制，不强制 Ctrl+C 中断任务 |
| Edge PDF/至少一种 PDF 阅读器 | 文本 PDF 选区；扫描 PDF 明确不支持 |
| 两屏 100%+150%/200%，副屏左置 | 位置、缩放、拖动、任务栏工作区、显示器拔插 |
| 中文用户目录、空格路径、只读位置 | 安装、资源定位、配置写错处理和更新引导 |
| 管理员程序、密码框、锁屏/恢复 | 失败安全，不模拟复制、不保留敏感或过期选区 |

证据包含 OS build、应用版本、构建 SHA、用例步骤、预期/实际、脱敏截图或录屏和必要错误码。每个应用明确标记 L1 自动 UIA / L2 显式复制 / 不支持。首版支持清单根据实测填写，不能用模型推断冒充测试。

性能目标为初始验收门槛：支持的本地文本 UIA 路径从稳定选区到图标 p95 ≤ 800ms；点击后本地加载态 ≤ 100ms；空闲 CPU 平均 ≤ 单核的 1%（固定测量环境记录方法）；主进程 + helper 空闲工作集目标 ≤ 150MiB。目标未达必须记录实测和处置，不能以网络首字时间代替本地响应。

## 11. AI 实施任务与完成条件

| 任务 | 前置 | 实施内容 | 完成条件 |
| --- | --- | --- | --- |
| G0 Windows 原型 | 无 | 锁依赖、最小编译、非激活透明窗、点击、托盘存活、DPI 转换 | 有真实桌面证据；失败则修订架构重审 |
| G1 平台拆分 | G0 | core/controller/ui/platform，macOS 逻辑平移 | Windows cfg 无 macOS 引用；原 macOS 关键行为回归 |
| G2 取词 | G1 | 事件触发、UIA helper、协议、超时和过期结果 | fake provider 故障测试 + 记事本/浏览器 L1 实测 |
| G3 交互闭环 | G2 | 图标、流式弹窗、拖动、复制、关闭、设置、热键 | 请求时机、焦点、乱序及 DPI 用例通过 |
| G4 桌面集成 | G1，最终依赖 G3 | 托盘、凭据、配置、单实例、自启动、朗读、显式复制 | 资源释放和失败降级通过，不更改 macOS 默认值 |
| G5 可发布首版 | G2–G4 | 用户级安装、签名、CI、手动更新、应用矩阵 | 支持清单、安装/卸载、macOS 回归和全部必选门禁通过 |
| G6 可选自动复制 | G5 | §6.2 独立实验能力 | 故障/竞争测试通过并重新评审；否则保持关闭 |
| G7 可选自动更新 | G5 | §9.2 签名验证、updater、恢复/回滚 | N→N+1、失败回滚、损坏包和中断恢复实测通过 |

允许 AI 对不重叠模块并行开发；必须各自使用独立分支/worktree，协调共享文件所有权。`Cargo.toml/Cargo.lock`、controller、协议和 Release workflow 由单一集成负责人合入。每个交接包含文件范围、协议版本、测试命令、真实证据、未解决项，不能以“工具调用成功”代替验收。

G0–G5 为首版完成条件。G6/G7 是明确后续能力，不得用其未完成冒称首版功能齐全，也不得让它们的风险偷偷进入首版默认路径。

## 12. 三方对抗评审与共识规则

评审者是实际本地 xiaok CLI、实际本地 Kimi CLI、当前 Codex；不得用同一模型扮演三个角色并宣称外部评审。CLI 使用已有默认配置，不打印凭据，不改变模型或全局配置。

- Round 1：三方独立读取同一个冻结方案和必要源码。检查前提是否成立、设计能否实现、竞态与数据损失、测试是否可证明、阶段是否偷减范围。
- Codex 汇总问题，但不能替外部评审者给出同意。为每项问题分配稳定 ID、严重性、证据、修订位置及状态。
- 后续每轮将修订方案、已处理问题和其他两方异议发给每位评审者，请其检查修复并主动反驳；发现新 blocker 继续修订，不能以轮数达到上限宣布一致。
- Blocking：会导致基本路径不可实现、错误翻译源、剪贴板/凭据损失、不可恢复安装或发布证据失真。Major：契约、职责和验收有实质歧义。Minor：不影响正确性的完善。
- 每轮材料保存于 `docs/windows/reviews/`，包含冻结快照 SHA-256、输入提示、原始评审输出、Codex 独立意见和问题台账。
- 达成一致要求：三方对相同最终快照明确 PASS；无未关闭 Blocking/Major；所有延期能力有明确不启用边界；运行验证项完整列为未来 gate，而非伪造已通过。
- 若任一 CLI 不可用、超时或未给出判断，记录事实并继续可做工作，不把沉默当同意，也不由 Codex 自己补造外部意见。

## 13. 外部依据及待验证假设

以下仅证明 API 和设计约束存在，不证明 BeeK 已实现或通过：

- [GPUI 0.2.2 平台源码](https://docs.rs/crate/gpui/0.2.2/source/src/platform.rs)：有 Windows 后端；非激活与 PopUp 具体语义需 G0 验证。
- [GPUI 0.2.2 原始 crate](https://static.crates.io/crates/gpui/gpui-0.2.2.crate)：检查过 Windows window 实现，不能只据上层 `focus:false` 推断行为一致。
- [Ely 当前 Cargo.toml](https://git.zacharyzhang.com/ZacharyZhang-NY/Ely-GPUI-Components/src/branch/main/Cargo.toml)：依赖与本地 BeeK GPUI 版本存在漂移，优先移除未使用依赖。
- [UIA GetSelection](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationtextpattern-getselection)：空选区可为退化范围，不能视作有效文本。
- [UIA 线程约束](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-threading)：独立 MTA 工作线程及事件注册/注销要求。
- [SendInput / UIPI](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput)：不能向更高完整性进程任意注入输入。

尚待实测：GPUI 非激活点击与渲染、托盘无窗口生命周期、混合 DPI 坐标语义、各应用 UIA 覆盖率、实际构建依赖、GPU 兼容性、语音可用性、签名与安装行为。三方设计共识不会将这些假设自动升级为事实。
