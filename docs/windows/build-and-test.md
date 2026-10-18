# Windows 构建、测试与内部打包

支持基线为原生 Windows 11 x64、本地登录的普通用户。主程序与 `beek-selection-host.exe` 必须来自同一次构建，放在同一目录；不依赖当前工作目录。macOS 原入口保留在 `src/macos_main.rs`。原设计与 G0-D 补充快照不作改写。

## 开发环境与构建

需要 Rust 1.99.1 MSVC、Visual Studio C++ Build Tools、Windows SDK 的 x64 FXC、Python 3.9+。找到 SDK 内的 `fxc.exe` 并设置 `GPUI_FXC_PATH`。仓库使用静态 CRT；仍须在无开发组件的干净机器验证实际运行依赖。

```powershell
cargo check --locked --all-targets
cargo test --locked --lib
cargo build --locked --release --bin beek --bin beek-selection-host
.\target\release\beek.exe --capabilities | Out-String
.\target\release\beek-selection-host.exe --capabilities | Out-String
.\target\release\beek.exe --verify-pair | Out-String
```

凭据事务的真实 Credential Manager 测试默认忽略，只在普通用户交互桌面执行（唯一测试命名空间，测试结束清理）：

```powershell
cargo test --locked --lib -- --include-ignored --test-threads=1
powershell -NoProfile -File scripts/windows/run-product.ps1 -BuildOnly
```

`run-product.ps1` 收集环境、编译 debug/release、运行包含真实凭据回滚的测试、记录能力及二进制散列、从不同 cwd 预检配对，并创建 G1–G5 证据模板。只有 Windows 构建项自动记为通过，其余保持 `not_run`。省略 `-BuildOnly` 才启动实际产品。SSH/CI session0 不能运行桌面验收。

## 故障与网络 harness

测试功能不进入正式构建。`test-fixtures` 生成额外测试程序及受限故障入口；打包脚本明确拒绝这种构建。harness 与 helper 同一配置构建。

```powershell
cargo build --locked --features test-fixtures --bins
python scripts/windows/mock_server.py --port 8765 --ready-file test-results/mock-ready.json
.\target\debug\beek-windows-harness.exe --help | Out-String
```

harness 实际参数使用 `--suite l1|fault|network|cancel-probe|clipboard-isolation`、`--source <HWND>`、`--output <JSON>`，按所选 suite 提供 `--fixture textarea|paragraph`、`--mode`、`--base`、`--count` 等参数。helper 必须位于同一目录并同构建。准确参数和限制以 [harness 源码](../../src/platform/windows/harness.rs) 为准。必须先在公开夹具中形成真实选区，不能在服务桌面伪造 HWND。`l1` 的重复同一选区实验不等于完整 L1 应用/手势矩阵或点击加载态性能验收。

桌面驱动必须由已登录普通用户在交互会话启动；它只控制工作区内 BeeK、明确创建的公开夹具窗口，拒绝向未知窗口输入。

```powershell
powershell -NoProfile -STA -File scripts/windows/desktop-agent.ps1 -Workspace C:\BeeK-Test -ProjectDirectory C:\src\beek
python scripts/windows/desktop_client.py --help
```

远程客户端支持 `--host ilucas`。所有桌面请求及响应保留 JSON，截图只允许公开测试窗口；不收集用户文档、请求正文或鉴权内容。驱动不是完整 UIA 自动验收，截图与实际原选区仍需核对。

普通网页回归要补充真实鼠标手势，不能只重复读取textarea。驱动的 `selection-gesture` 接受 `mode: double|drag`、窗口内的 `x/y` 以及拖动的 `end_x/end_y`；只允许当前前台公开夹具。覆盖从文字内开始、松于远处空白，以及从空白开始、松于文字内的正反拖动及中文；`selection-gesture` 的 `drag` 支持 `duration_ms`（12–5000）和 `pause_ms`（0–2000，中途停顿），补测慢拖及停顿后继续拖动，按住期间不应消费最终选区检测；等待超过500ms再点蜜蜂，验证点击复核仍命中源文字。点击前服务请求数必须不变，点击后恰增加1；空白点击和密码区域不应弹出按钮或联网。蜜蜂点击回归必须包含真实按住时长：桌面驱动的 `click` 支持 `hold_ms`（0–2000ms），至少覆盖180ms和600ms；松开后在250ms、1s、3s检查卡片持续显示、蜜蜂已消失且源窗口仍在前台，不能只检查按下后的首帧。固定公开窗口几何，任何实际窗口移动/恢复导致坐标失效的记录必须标为inconclusive，不能据此声称通过或产品失败。

## 安装包

使用 Inno Setup 6.7.3（本次验证的安装器 SHA-256 `9c73c3bae7ed48d44112a0f48e66742c00090bdb5bef71d9d3c056c66e97b732`）。下载同时校验签名和散列；工具使用需遵循其[许可说明](https://jrsoftware.org/ishelp/topic_purchase.htm)。编译器在本次机器显示 Non-commercial use only，不能据此声称已获得商业许可。

```powershell
python scripts/windows/licenses.py --strict --out test-results/THIRD-PARTY-NOTICES.txt
powershell -NoProfile -File scripts/windows/package.ps1 -Iscc 'C:\Tools\Inno Setup 6\ISCC.exe' -OutputDirectory dist\windows-preview
```

这产生 **internal-preview-unsigned** 的 zip/setup 与散列，不能正式发布。正式模式还需 `-Formal -PublisherThumbprint <预期证书指纹> -SignTool <signtool.exe>`；私钥/密码只供受保护 CI。校验主程序、helper、setup、签名卸载器以及完整第三方声明。Inno 的 `x64os` 用于原生 x64 范围，[架构标识说明](https://jrsoftware.org/ishelp/topic_archidentifiers.htm) 区分模拟兼容架构。

安装器为用户级、不要求提升，放入不可变版本目录。运行中持锁阻止覆盖，不强杀程序；默认卸载保留配置和凭据。显式清理用户数据具有删除效果，不得把生产凭据拿来试验。内部安装测试使用专有目录，保留安装日志和注册表前后记录。

## 验收与发布

[完整测试方案](windows-test-plan.md) 与 [87项用例](../../tests/windows/cases.json) 是验收依据。`evidence.py` 要求实际环境、能力元数据、源码/二进制/证据散列；缺任一阶段或未跑用例都会拒绝。

```powershell
python -m unittest discover -s tests/windows -p 'test_*.py' -v
python scripts/windows/evidence.py --help
python scripts/windows/release_assets.py --help
```

GitHub Windows CI 在 main 推送、PR 和手动运行时执行格式、Rust/工具测试以及 release 打包。统一 Release workflow 等待 macOS 与 Windows 两端成功，校验 zip/DMG/setup 和 SHA256 后一次性发布。用户于2026-10-18明确要求对外下载，因此现阶段以 `-PublicPreview` 生成标注为 **public-preview-unsigned** 的 Windows 公开预览包，不再依赖尚未完成的正式验收附件来发布预览。默认仍拒绝把 internal-preview-unsigned 内部包作为公开预览发布。签名模式可通过 Release 的 `sign_windows` 或仓库变量 `WINDOWS_SIGN_RELEASES=true` 启用，并要求签名 secrets；完整 G0–G5 标准和证据校验工具保持原状，公开预览不计为正式验收通过。没有推送或执行过的 workflow 不能记作 CI 通过。当前实际完成及外部环境缺口见 [实施状态](implementation-status.md)。
