# BeeK Windows 完整测试方案与执行手册

状态：Windows 产品、独立 helper、故障 harness、安装器和测试工具已实现，正在 ilucas 做真实桌面回归；完整 G0–G5 尚未全部通过。不能用 macOS 编译或 CI 绿色代替交互桌面验收。实施进度见 [implementation-status.md](implementation-status.md)，执行入口见 [构建测试手册](build-and-test.md)。已通过的设计快照保持原文，见 [原方案](windows-port-plan.md) 与 [共识](reviews/consensus.md)。

## 1. 范围和通过定义

首版覆盖 G0–G5。G6 自动模拟 Ctrl+C 与 G7 自动安装更新仍是后续可选能力，首版必须验证它们未被启用。测试不使用真实 API Key、个人文档、密码或客户剪贴板；网络使用 loopback 假服务，应用使用仓库公开夹具。

[机器可读用例](../../tests/windows/cases.json) 定义 **87 个必测用例**，每项含稳定 ID、阶段、执行方法、步骤、预期和证据要求：

| 阶段 | 数量 | 主要覆盖 | 当前执行条件 |
| --- | ---: | --- | --- |
| G0 | 18 | 编译、非激活透明窗、点击、输入/IME、DPI、F11、托盘驻留/恢复/退出 | 独立原型；需要 Windows 11 交互桌面 |
| G1 | 4 | Windows/macOS locked 构建、依赖锁、平台隔离 | 产品平台拆分完成后 |
| G2 | 20 | UIA、来源/选区、协议、挂起/崩溃/隔离、满载、剪贴板竞争、Job | 真实 helper 与故障注入 harness 完成后 |
| G3 | 15 | 点击前零请求、过期复核、流式错误/上限/取消、焦点/拖动/IME | 产品交互闭环完成后 |
| G4 | 15 | 默认值、凭据事务、配置、IPC、单实例/重启、自启动、语音/隐私 | 桌面集成完成后 |
| G5 | 15 | 安装升级卸载、签名、双平台资产、最小 L1、性能、稳定性/回归 | 可安装构建完成后 |

每项状态只能是 `not_run`、`pass`、`fail`、`blocked`。没有 N/A、默认为通过或“观察正常即可跳过”的路径。支持范围改变需要修订方案及用例并重审，而不能删掉失败用例。G0–G5 全部通过才有首版完成证据；任何阶段单独通过不代表产品完成。

门禁检查实际结果、证据文件散列、被测 exe 散列、源码指纹、用例目录散列及环境。手工桌面项目要求截图/录屏；仅一段声称通过的文字不能通过证据检查。它检查记录完整性，不会自动理解视频中 IME 是否正确，也不能替代评审者核对真实画面。

## 2. 环境矩阵

| 环境 | 用途 | 必要记录 |
| --- | --- | --- |
| Windows 11 x64，普通用户，本地登录桌面 | 正式必测基线 | OS build、原生架构、会话、Rust/MSVC/SDK、GPU/驱动、屏幕/DPI |
| 同上双屏，100%+150%，再换 100%+200%，副屏放左侧 | 几何、输入候选、负坐标、热插拔 | 每屏物理边界/工作区/DPI、录屏 |
| 同上中文账户名、空格安装路径 | 配置/资源/安装升级 | 路径脱敏记录及实际结果 |
| 干净用户/无 Rust 无开发组件 | zip/installer 依赖完整性 | CRT/图形依赖、安装器日志、版本 |
| GPU 不同厂商/驱动的第二台可用 Win11 x64 | 渲染兼容性 | GPU/驱动及失败回退结果 |
| macOS 当前受支持环境 | 原平台回归 | 原功能默认值、构建/自检和旧资产更新协议 |
| GitHub windows-2025/Ubuntu runner | 构建/纯自动测试 | 仅 CI 证据，不能证明 UIA/焦点/IME/托盘交互 |
| 管理员目标、密码框、锁屏、安全桌面 | 排除项失败安全 | 不提权、不注入、不读敏感文本、不留下旧窗 |

Win10、ARM64、远程桌面、游戏和独占全屏不列为首版支持。若顺带测试，须单独记为探索结果，不得充当 Win11 x64 交互桌面证据。

Windows 开发环境需要 Rust 1.99.1 MSVC 工具链、Visual Studio C++ x64/x86 Build Tools、Windows SDK（含 x64 `fxc.exe`）和 Python 3.9+。GPUI **release** 构建使用 FXC 编译 HLSL，不能只安装 DXC 或只验证 debug 构建。G0 crate 的工具链与依赖均固定；验收期间只允许 `--locked`，不得临时升级框架解决问题。

`collect-environment.ps1` 采集 OS/架构/会话、SDK/FXC、GPU、显示器和 Rust 信息，拒绝 ARM64、服务会话及低于 Win11 的 build；不枚举全部环境变量、不收集凭据。双屏布局、驱动差异、锁屏/Explorer 操作和实际输入法仍需人工核对。

## 3. 仓库中的执行工具

| 文件 | 能力/界限 |
| --- | --- |
| [tools/windows-probe](../../tools/windows-probe/README.md) | 不依赖主应用的 GPUI/Win32 G0 原型；不是 Windows 产品 |
| [run-g0.ps1](../../scripts/windows/run-g0.ps1) | 环境采集、locked debug/release/test、复制被测 exe、初始化记录、启动原型 |
| [evidence.py](../../scripts/windows/evidence.py) | 创建/记录/校验每阶段证据，以及聚合 G0–G5 门禁 |
| [trace_check.py](../../scripts/windows/trace_check.py) | G0 可观测焦点/点击/无窗驻留不变量；不替代视觉/IME判断 |
| [mock_server.py](../../scripts/windows/mock_server.py) | 仅 loopback 的正常/分片/慢流/断流/超限/HTTP 错误服务；不记录输入或鉴权 |
| [metrics.py](../../scripts/windows/metrics.py) | 所有尝试的分母、成功/超时率及 p95；缺样本/失败不能被过滤 |
| [measure-idle.ps1](../../scripts/windows/measure-idle.ps1) | 采集主进程与全部 helper 的 CPU/内存/句柄/线程；单核口径 |
| [selection.txt](../../tests/windows/fixtures/selection.txt) / [selection.html](../../tests/windows/fixtures/selection.html) | 记事本、Edge、Chrome 使用同一公开中英文/emoji/多行夹具 |
| [test_tools.py](../../tests/windows/test_tools.py) | 工具本身的负向测试；不是 Windows 运行验收 |
| [windows-g0.yml](../../.github/workflows/windows-g0.yml) | 可人工触发的构建/工具 CI，明确不签核交互桌面 G0 |

产品端诊断、受限桌面驱动、真实协调器/网络 harness、产品 runner 和打包入口已实现，见 [构建测试手册](build-and-test.md)。已有单项实测不替代下文完整矩阵，也不能自动填满87项。

## 4. G0 的实际执行流程

### 4.1 启动

在 **Windows 11 x64 的本地登录桌面**打开 PowerShell，切换至此仓库根目录，确保 Rust/MSVC、SDK、Python 可用。不要从 Windows 服务/CI agent 启动桌面验收。

```powershell
powershell -NoProfile -File scripts/windows/run-g0.ps1
```

脚本创建新的 `test-results/g0-<timestamp>/`，其中有环境 JSON、构建/测试日志、被测 exe 副本、`run.json` 和原型 JSONL。关闭原型时必须使用通知区的 Exit；直接杀进程不能形成正常退出通过证据。

只验证构建可以运行：

```powershell
powershell -NoProfile -File scripts/windows/run-g0.ps1 -BuildOnly
```

`-BuildOnly` 不启动 UI，除构建用例外其余全部维持 `not_run`，G0 不能通过。

### 4.2 操作与必须观察的结果

1. 原型输入窗口中，只输入仓库公开夹具。测试 ASCII、中文、emoji、左右/Home/End/Delete、鼠标选择、Ctrl+A/C/V/X。
2. 测试微软拼音中英切换、组合文本下划线、候选窗位置、提交/取消、在非零偏移选区中替换带 emoji 的文本；录像包含候选窗与插入位置。
3. Tab/Shift+Tab 在两个字段切换；先鼠标点另一字段再按 Tab，检查按实际焦点切换。
4. 点击“3 秒后显示非激活透明窗”，立即切回有公开选区的记事本。检查窗口出现前后记事本仍为前台、原选区保留。透明窗设置发生在首次显示前，不能接受先抢焦点再恢复的闪动。
5. 点击悬浮窗至少 10 次，观察计数递增而记事本的焦点和选区保持；录屏包含两者。轨迹检查只证明原型没激活，记事本实际选区仍须观察。
6. 在明暗背景、多屏、100%+150%/200%、左置副屏分别重做展示/点击/输入。检查透明背景、命中区域、位置及候选窗，不只看数值日志。
7. 测试拖动输入窗口跨屏，拔除原显示器后继续输入；不能出现不可访问窗口。
8. 在 Edge 公开网页中 F11 进入/退出全屏后重测显示/点击，不能抢焦点、退出全屏或闪烁；不能把普通最大化误当不支持。
9. 从通知区右键选“Close all probe windows”关闭全部窗口（同时取消延迟展示），等待至少 **35 秒**，再从通知区打开输入窗；日志需要至少 30 秒跨度的 `heartbeat visible_windows=0 / gpui_windows=1`。原型没有实现产品的崩溃自动拉起。
10. 在专用测试桌面手动重启 Explorer，确认通知区图标重建且只有一个；不由脚本擅自重启桌面。
11. 从通知区退出，再次启动原型；检查没有残留进程或图标。
12. 保存各用例截图/录像，并检查 JSONL 未包含输入文本或剪贴板正文。不要在截图中放真实凭据。

### 4.3 写入与校验结果

将截图、视频和命令输出放在本次 run 目录中，路径不能逃出该目录。例如为 `G0-04-CLICK` 记录结果（其余 ID 以 cases.json 为准）：

```powershell
python scripts/windows/evidence.py record --run test-results/g0-实际时间/run.json --case G0-04-CLICK --status pass --actual "10次点击计数正确；记事本前台及选区均保持" --evidence evidence/click.mp4 --evidence logs/probe.jsonl
python scripts/windows/trace_check.py test-results/g0-实际时间/logs/probe.jsonl
python scripts/windows/evidence.py validate --run test-results/g0-实际时间/run.json
```

失败写 `--status fail` 并说明实际错误；环境不足写 `blocked`，不能改成 pass。自动构建项可由 runner 写入通过；其他项目不能只因 trace_check 成功批量改成通过。

任何 `not_run/fail/blocked`、环境不符、证据缺失/变化、二进制缺失/变化、源码或用例目录变化都会使校验退出非零。更改代码后保留旧记录并新建一轮；不得重写旧证据来维持绿色。G0 全部通过后，才允许按已评审方案进入依赖它的 G1。

## 5. G1–G4 的开发测试约定

以下是产品必选验证。哪些已经执行以各轮原始证据和实施状态为准，未执行项目不得标为通过。

### 5.1 平台与核心测试

G1 完成后在 Windows 和 macOS 分别执行应用 `cargo check --locked --all-targets`、`cargo test --locked` 和对应 release 构建。确认 Windows 无 objc/AppKit 业务引用，macOS helper 的入口条件正确，资源定位不依赖 cwd。提交 root Cargo.lock；不引入未使用/未固定的 Ely。

核心状态机应接受可控时钟、fake source、fake worker、fake transport 和 fake credential store，以测试真实契约而非墙钟睡眠或实现私有字段。窗口外观/UIA本身仍需实机；纯模型状态机测试不能为焦点/IME签核。

### 5.2 UIA 与 helper 故障注入

G2 的产品 harness 需提供：固定来源 token/创建标识/generation、请求 ID、操作开始/完成/回收时刻、worker 活跃/终止待确认数量、创建尝试时间、退避状态、结果枚举；默认不输出真实正文。公开夹具可以断言文本哈希/长度、矩形和来源。不得接受“整页文本与所选字串恰好相似”代替准确选区。

必须注入：

- 操作永久阻塞、调用失败、进程崩溃以及仅订阅回调崩溃，均有来源归属；1/5 秒退避、第三次 30 秒暂停、第四次探测失败隔离后不再自动自旋。
- 两慢来源 A/B 后切健康 C，回收最早失效占用者；终止未确认者仍计入 2 个 UIA worker 上限；无可回收槽仅一个最新 pending，最多等 300ms，Busy 不误记健康源故障。
- 入队起 1500ms 总预算包含等槽/回收/启动/操作，取得槽不重置计时；所有 helper 任意滚动 10 秒创建尝试不超过 6 次，启动失败也算。
- 单坏源不能触发全平台停用；跨源真实协议故障以及无来源启动/握手故障分别验证平台/基础设施暂停。
- reader 阻塞不占 UIA 槽或持有其订阅；单 reader、单最新 sequence、300ms 过期自动请求不复活、显式重试重绑 sequence。
- 错版本、错 token、错来源/乱序/旧 generation、异常长度、断管；主进程强杀后的 Job 清理、锁释放和手动重开。

真实 COM provider 挂起和模拟挂起都需覆盖。故障测试用例中的预期 Busy/Protected 不当成正常 L1 成功；普通性能集合与故障集合分开标记，但两者都完整保留统计。

### 5.3 翻译与假 SSE 服务

在一个测试终端运行：

```powershell
python scripts/windows/mock_server.py --port 8765 --ready-file test-results/mock-ready.json
```

服务仅监听 `127.0.0.1`，无外网请求。产品可替换测试端点后，使用公开 key `public-fixture-key`。不同用例直接将 **完整请求端点**设为以下路径，不要把 `/chat/completions` 再拼在后面：

| 路径 | 场景/断言 |
| --- | --- |
| `/sse/normal` | 正常中文/emoji，分片合并准确，最终完整正文 |
| `/sse/fragmented` | 单字节网络写入，UTF-8 与 JSON 边界不损坏 |
| `/sse/disconnect` | 有部分正文但没有 DONE；明确不完整，不当成功 |
| `/sse/slow` | 每秒增量持续 65 秒，验证总超时与真实工作槽 |
| `/sse/empty` | 完成但无正文，有明确反馈 |
| `/sse/oversized-frame` | 大于 64KiB 的帧，读取中拒绝，不先无限分配 |
| `/sse/oversized-output` | 累计大于 256KiB，终止读取并明确报错 |
| `/http/401`、`/http/403`、`/http/429`、`/http/500` | HTTP 错误含公开哨兵及长正文，验证限长/脱敏 |
| `/nonstream` | 非流式公开响应，供兼容自检 |
| `GET /stats` | 只返回请求计数、活动和峰值数，验证显示图标/预览前后零请求 |

测试不会把请求正文或 Authorization 写到日志。服务最多 8 个活动 fixture 请求，超额返回503；单请求体上限128KiB。超长/慢服务是测试故障，不应配置到日常真实用户配置中。

产品须验证两个真实 HTTP 槽、一个最新 pending、重复点击去重、关闭只逻辑取消、旧响应不复活、UI背压有界、退出不等60秒。请求配置快照在开始时冻结，不能混用改动前后的 key/model/base_url。

### 5.4 配置、凭据及生命周期

Credential Manager 与配置文件不能以“save成功一次”作为事务证明。注入新引用写失败、读回失败、原子配置替换失败、提交前/后进程中断、旧凭据删除失败，核对至少一个完整已提交组合保持可用；凭据读取失败不切其他账户。损坏配置备份、默认值、字段保留和并发更新另测。

命名管道覆盖错误SID/会话、服务器PID/创建标识不符、抢占/伪造端点、未知消息、4KiB上限和1秒超时。普通第二次启动只 `ActivateSettings`，不能误入重启协议。重启覆盖创建失败、预就绪失败、父不退出、父已退出但子失败及PID复用；不能出现两个主实例或无反馈完全退出。

## 6. G5 产品验收

### 6.1 应用与桌面覆盖

最小 L1 固定为 Windows 11 记事本、Edge普通网页、Chrome普通网页。每个应用分别记录版本，执行中英拖选、双击、跨行、键盘选区、空选区、输入区和点击后选区不丢失。任何一项失败都阻止G5，不能把该应用改成L2逃避。

扩展矩阵包括 Word、VS Code/Electron、Terminal、Edge文本PDF/另一PDF阅读器、扫描PDF、Edge F11/放映、管理员程序/密码框/锁屏。每个明确L1/L2/不支持；扫描/不可选画面不得返回全文，Terminal不得强发Ctrl+C中断任务。双屏、输入法和无窗口常驻在产品上重测，不能仅援引G0原型。

### 6.2 性能与稳定性

选择尝试 CSV 格式：

```csv
app,scenario,outcome,latency_ms,loading_ms
notepad,regular,success,220,25
edge,fault,timeout,1500,
```

`app` 使用 `notepad/edge/chrome` 等稳定名称；`regular` 为正常非空公开选区，`fault` 为明确故障注入。时间来自产品单调时钟：选区稳定到按钮、点击到本地加载态，不能拿网络首字时间代替。所有 Busy/timeout/error 必须有实际耗时；不能删除失败行。

```powershell
python scripts/windows/metrics.py test-results/attempts.csv --enforce
powershell -NoProfile -File scripts/windows/measure-idle.ps1 -OutputFile test-results/idle.csv
```

每个最小L1应用至少100次正常非空尝试及100次加载态记录；正常路径失败需先调查和修复，不能以p95掩盖。完整尝试 p95≤800ms，本地加载态每次≤100ms。另报告故障集合成功/超时率及按来源分布。空选区正确返回另有功能用例，不混入性能成功数。

空闲测量至少300秒，统计单核口径CPU（不除以逻辑核心数），主进程加全部helper工作集、句柄与线程。目标CPU平均≤1%、总空闲工作集≤150MiB；报告实际worker数、测量环境及样本。`measure-idle.ps1`用于稳态采样，不能完整捕获两采样间已退出的短命进程；频繁进程故障用精确worker事件或ETW补证，不得用空闲采样替代创建率验证。

执行8小时稳定性循环，覆盖切源、复制、网络断开/恢复、关闭/重开、休眠恢复；保存周期资源、错误码与故障时间线。测试失败/崩溃后应保留证据，不重启后仅报告最终状态。

### 6.3 打包、升级与发布

新装、同版本重装、N→N+1、运行中升级同意/拒绝/退出失败、取消、卸载保留/显式清理全部覆盖。验证中文账户、空格路径、不同cwd、无开发工具机器、资源/CRT/GPU依赖、exe/helper版本一致。拒绝退出时安装停止，不能强杀后覆盖半套文件。

正式发布前核验主程序/helper/setup的Authenticode和预期发布者；缺签名只能是内部预览，不能通过正式发布。双平台聚合资产缺任一项必须拒绝：macOS通用zip/DMG、Windows x64 zip/setup。旧macOS精确资产名保持兼容；Windows无包、错误架构/通道必须正确反馈。首版更新只开固定HTTPS发布入口，不自动执行任意URL。

G5报告还需附macOS关键回归。签名凭据只供受保护发布流程，PR/普通CI没有密钥；不把证书或真实API Key附进测试证据。

## 7. 证据归档与最终门禁

每轮一个新目录，建议：

```text
test-results/<run>/
  run.json                   # 用例状态、源码指纹和所有证据散列
  environment.json           # OS/架构/会话/工具链/GPU/显示器
  artifacts/                 # 实际被测的exe副本，包含helper（G1以后）
  logs/                      # 命令日志、脱敏JSONL、安装日志
  evidence/                  # 每个手工用例截图/录屏
  attempts.csv / idle.csv    # 性能原始样本
```

G1–G5初始化时需提供真实 `beek.exe` 与 `beek-selection-host.exe`，不得拿probe改称产品通过。所有阶段在最终冻结源码上复跑或证明对应变化重新验收；当前校验器采用严格的完整源码指纹，任一源码变化要求新的记录。

```powershell
python scripts/windows/evidence.py create --stage G2 --out test-results/g2/run.json --environment test-results/g2/environment.json --artifact target/release/beek.exe --helper-artifact target/release/beek-selection-host.exe --capabilities test-results/capabilities.json
python scripts/windows/evidence.py release-gate --run test-results/g0/run.json --run test-results/g1/run.json --run test-results/g2/run.json --run test-results/g3/run.json --run test-results/g4/run.json --run test-results/g5/run.json
```

聚合门禁要求G0–G5各一份当前有效通过记录；重复同阶段不能填其他阶段。它不会自动发布。安装签名、资产纪律以及受保护发布工作流本身还需通过相应检查。

工具自身回归：

```sh
python3 -m unittest discover -s tests/windows -p 'test_*.py' -v
cargo test --manifest-path tools/windows-probe/Cargo.toml --locked
```

这些命令的成功只证明相应代码/工具测试，不能由它们生成“Windows实机全部通过”的结论。

生命周期适配说明：GPUI0.2.2 Windows在最后一个框架窗口销毁时会PostQuitMessage。原型使用公开API建立一个永久1×1且show=false/focus=false的生命周期宿主；它不显示、不进任务栏，明确只在退出时释放。日志分开记录visible_windows和gpui_windows，不能用隐藏宿主冒充零个框架窗口。
