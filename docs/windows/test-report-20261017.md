# 2026-10-17 Windows 实测报告

本报告保留首批执行结果；资源恢复后的晚间追加复测见[追加报告](test-report-20261017-recheck.md)，原失败不改写。

Windows 实现已落地，并有可下载到本地的未签名内部包。完整 G0–G5 尚未通过，不能正式发布。所有测试只使用公开夹具、loopback假服务和假API Key。原始数据在 `test-results/ilucas/` 与 `test-results/host/`，不提交包含机器路径的临时截图/日志到公开发行。

## 环境及版本

ilucas：Windows11 Pro build26300、原生x64、RTX4090D、单1920×1080/96DPI，Rust1.99.1、MSVC19.50、SDK26100。桌面测试在本地登录普通用户session1；SSH的session0只作传输/编译/运维预检。

最终内部包 build ID：`69e09ecbc3a9ca689a5b613564e67c7e8c6cd25205936a6b318ceab864d71d49`。`debug_assertions=false`、`test_fixtures=false`；package脚本拒绝fixture及不匹配架构/配对。

## 实际结果

| 检查 | 结果 | 原始证据/限制 |
| --- | --- | --- |
| G0-D原型release | 15项通过 | g0-development-gate.json；不代表完整18项G0 |
| Windows locked check/test/debug/release | 多轮成功 | product-test-final.log、product-release-final.log；后续R5增量release也成功，最终runner额外重复链接因低内存中止 |
| Windows真实交互Rust测试 | 23/23、原生exit0 | product-tests-interactive-final.json，包括真实Credential Manager/原子替换失败/过期保存 |
| macOS all-target检查、核心测试 | check通过，19/19 | host/product-mac-check-final.log、product-core-test-final.log；使用dev-runtime-shaders，无完整Xcode运行验收 |
| Python工具 | 22/22 | host/windows-tool-tests.log，覆盖缺证据、过期源码、资产/签名/流式上限负例 |
| 格式、工作流、脚本 | fmt、diff检查、Ruby YAML解析、Windows PS语法解析通过 | workflow未推送/运行，不能称CI通过 |
| UIA正常重复读取 | Notepad100/100，修复后Chrome新profile100/100，Edge100/100 | debug fixture helper，完整分母；Edge首轮705ms，Chrome首轮284ms；不是完整L1手势/加载态矩阵 |
| 真实协调器故障 | 查询/回调挂起、1/5/30退避、第四失败隔离、取消恢复探针、reader隔离通过 | harness-fault-*.json、harness-cancel-probe-*.json、harness-clipboard-isolation-*.json |
| 实际网络边界 | 12场景通过 | harness-network-query-hang.json，含两条真实慢请求到60秒、最新pending替换/取消、分片、断流、HTTP及上限 |
| 字段剪贴板 | 双/三连粘贴、紧接剪切粘贴、真实持锁失败保全通过 | product-editor-regression.json；旧弱断言保留为initial-inconclusive |
| 产品IME | 非零偏移emoji替换、中文提交/取消通过 | product-ime-regression.json，截图经过查看 |
| 实时结果卡 | 复制准确、焦点/选区保持、自复制不回流、持锁时失败反馈通过 | product-copy-echo-and-failure.json；原始按钮不可见失败另存initial-failure |
| 单实例、手动重启 | 第二启动只激活Settings；真实父子交接通过 | product-lifecycle-final.json，旧35984→新35456，debug fixture构建 |
| 系统语音 | release返回“朗读完成”，原选区保持 | product-release-speech-observation.json；不声称远程验证实际扬声器发声或无语音故障路径 |
| 内部安装 | 中文+空格路径安装、不同build同版本升级、同版重装、运行中拒绝安装 | product-install-final-3.json；非中文账户/非N→N+1/非取消或签名验收 |
| 默认卸载与Run清理 | 配置SHA逐字不变、真实凭据保留、已属本安装的引号Run项和卸载注册移除；再安装通过 | product-uninstall-preserve-final.json，凭据前后核查均在session1返回Win32 error0；SSH登录的CredRead不可作权威结果 |
| 配对与依赖 | 缺helper/混用返回3，同配对不同cwd返回0；主/helper无动态CRT导入 | product-pair-and-imports-final.json对应前一轮release，最终003安装目录能力/散列/不同cwd配对另由product-installed-preflight-final.json核实；不替代干净机运行 |
| 旧release空闲 | 约307秒，观察单核CPU0%，最高约51.9MiB | idle-product.csv；当时主进程1、helper0，不作为最终暖机后的完整结论 |

## 未通过与未完成

最终003 release安装成功，但随后暖机取词出现helper启动/操作超时。当时32GB机器的可用内存降至约50–265MiB，BeeK工作集被压至约5MiB。已保留原始Fault/桌面请求，停止本轮额外cargo/rustc/link子进程树；没有结束其他程序，也没有更改超时门槛或把失败改通过。当前无法据此确认低内存是唯一原因，须在资源恢复后重跑最终release及冷启动L1。额外产品runner首次有PowerShell同名Cargo函数递归，已改为Invoke-Cargo/cargo.exe，失败记录保留；二次runner的check完成，重复test链接中止，未记整体通过。

Explorer检查虽未发现文件夹窗口，但发现可见的非shell窗口，受限驱动拒绝重启，故未执行。多屏/混合DPI/负坐标/拔插缺设备；无可用代码签名证书；干净无开发工具环境、第二GPU、完整L1及故障组合、100次加载态、最终5分钟暖机空闲、8小时稳定性、安装取消/N→N+1/显式清理、完整macOS运行与签名回归均仍待验收。

87项用例保持必测，未批量标记PASS，没有N/A。正式发布门禁应继续拒绝；[测试手册](windows-test-plan.md)列出逐项步骤和证据要求，[构建手册](build-and-test.md)提供执行入口，[源码共识](reviews/implementation/consensus.md)只证明已审范围内的源码问题关闭。

## 内部交付

本地 `dist/windows-preview-20261017/`：

- `BeeK-v1.0.14-windows-x64.zip`：SHA256 `639c3904671bbf5a9fbb5ea245ca61a2eada1f7e33d373790ce9202882fa3b22`。
- `BeeK-v1.0.14-windows-x64-setup.exe`：SHA256 `0aef794f4aee9fa1f5a3c62225645b99d483046359e8c254d05dcd94e09b948e`。
- 对应 `.sha256` 与 `manifest.json`；已逐文件核对zip内manifest散列。

它们为internal-preview-unsigned。最终保留内部预览安装、不自动运行；测试创建的假key/loopback配置保留，实际使用需在Settings配置自己的服务。公开夹具浏览器与假服务仅按工作目录归属清理，不触及用户原有浏览器。无提交、推送或公开发布。
