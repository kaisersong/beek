Session: sess_4a763d88-ca54-4e3a-a975-54944c197eea

# 评审结论：有限 PASS（仅限本附件所列子系统）

附件已完整读取（811 行，无截断）。按要求只针对四个焦点做对抗性核查，未使用其他工具或外部文件。**未发现 Blocker 或 Major 级可触发 Bug；发现 7 项 Minor。** 同时明确：未签名预览包、缺外部实测等保留发布阻塞不计入代码 Bug，本结论不构成 G5 通过。

---

## 焦点一：HTTP 并发槽 / 总截止 / 排队 / 迟到回包 — 通过

- 槽位语义属实：`Pool::new`（stream.rs:65）恰好 2 个真实阻塞 HTTP worker；`pending` 为单槽“最新者胜”（stream.rs:174-182），被替换请求的 mailbox 立即落 `Done(Err)`，不会悬挂。
- 总截止覆盖排队：deadline 在 `submit` 时设定（stream.rs:177，now+60s），排队等待计入；`send` 用 `checked_duration_since` 求 remaining 并把 remaining 作为请求级总超时（stream.rs:229-235），reqwest 请求级 timeout 覆盖到 body 读毕，慢速 SSE 不会挂死。
- 迟到回包安全：watcher（stream.rs:132-167）与 worker 终写（stream.rs:109-120）都经 `reported` 的 CAS 争终点，恰好一方写 `Done`；超时后 worker 的 `Text`/`Done` 均被 `reported` 门禁拦下（stream.rs:100-103、110），不会覆盖超时错误。`Loading` 写入同样带门禁。
- `drop(client)` 可能阻塞 DNS 的尾巴被显式排在结果发布之后（stream.rs:121-128），期间 Active 残留项因 `reported=true` 不会再被 watcher 误报。
- 每个 mailbox 必然终态化：pending 路径（替换/超时/被取走）与 active 路径（worker 终写或 watcher 超时）闭合，无永久 Queued。

## 焦点二：SSE 有界与错误隐私 — 通过

单行 ≤64KiB+1（take 截断检测）、事件帧 ≤64KiB、整流 ≤4MiB、译文 ≤256KiB 四层上限齐全（stream.rs:249-281、311-314）；未终检 `[DONE]`/`finish_reason` 前中断报“提前中断”，部分内容不会伪装成功（有测试覆盖）。错误字符串均为固定文案或仅含 HTTP 状态码，不回显 endpoint、key、响应体或 SSE error 内容。

## 焦点三：Windows 更新精确资产与固定 URL — 通过

`windows_update`（release.rs:395-417）对 zip 与 setup.exe 双资产做“名称精确 + `browser_download_url` 与固定规范 URL 全等”校验，任何偏差 fail-closed 报错而非放行；draft/prerelease 拒绝；`check_update`（integration.rs:478-527）响应体限 1MiB+1、禁重定向、打开的是硬编码 releases 页面，从不执行 API 返回的 URL。

## 焦点四：安装器竞态与签名门禁 — 通过（含 Minor）

- 锁时序正确：`PrepareToInstall`（beek.iss:769-776）在复制任何文件前取 `install.lock`（share=0、CREATE_ALWAYS），失败即中止且提示“未替换任何文件”；并发安装器/卸载器互斥成立。取消路径 `DeinitializeSetup`（beek.iss:788-789）必关句柄。
- 升级切换时序正确：快捷方式与 Run 项仅在 `ssPostInstall`（beek.iss:777-787）即两枚不可变版本化 exe 完整落盘后切换；Run 项改写带 `Pos(...)=1` 前置判断，不碰用户自定义路径。
- 运行中检测靠 `AppMutex`/`CheckForMutexes('Local\BeeK-Running')` 双向把关，卸载不强杀进程。
- 签名门禁闭环：`-Formal` 强制 thumbprint+signtool（package.ps1:619）、签名后 `Get-AuthenticodeSignature` 校验（package.ps1:630-633）、attestation 仅 Formal 写出（package.ps1:643），`release_assets.py` 默认 formal 模式逐文件核验 status/thumbprint/sha256 与 zip 内实hash；`--internal-preview` 是唯一旁路且显式。

---

## 发现清单（均为 Minor）

1. **SSE 单行边界 off-by-one** — stream.rs:251-257 `parse`。触发：服务端某行内容恰为 65536 字节且以 `\n` 结尾时，`n = FRAME+1` 即被拒，报“单行超过64KiB”，但内容并未超过。最小修复：仅当 `n == FRAME+1 && line.last() != Some(&b'\n')`（截断）时判超限。
2. **更新结果通道可错位/阻塞 worker** — integration.rs:462-477 `open_updates` / :528-532 `update_result`。触发：第一次检查完成后用户关闭轮询 UI（结果滞留 `out` 缓冲），再触发检查时 worker 阻塞在第二次 `out.send`（integration.rs:469）；期间 UI 首次轮询取到的是上一轮旧结果，可能把旧结论当作本次结论展示；若 UI 永不再轮询则更新检查功能假死至重启。最小修复：`out` 改用 `try_send` 丢弃旧值（新结果覆盖），或发送前 drain。
3. **开发版版本号使更新检查直接报错** — release.rs:400（`version(current).ok_or("当前版本格式无效")`）。触发：Cargo.toml 带预发布后缀（如 `-dev`）的本地构建点“检查更新”，用户看到“当前版本格式无效”。最小修复：当前版本解析失败时按 `[0,0,0]` 处理，继续对照已校验的稳定资产。
4. **卸载收尾锁释放与目录删除之间有竞窗** — beek.iss:808 `usPostUninstall`。触发：`CloseHandle`+`DeleteFile` 之后 Inno 清理 `{app}` 残留期间，新安装器可成功取锁并开始写入，随后文件被卸载收尾删除，产生残缺安装。窗口极短。最小修复：锁句柄保持到卸载进程退出前最后释放（去掉 usPostUninstall 的提前 CloseHandle，仅依赖 `[UninstallDelete]` 清文件）。
5. **Run 项路径形态不匹配导致改写/删除失配** — beek.iss:783、806 的 `Pos('"{app}\', Old)=1` 判断。触发：应用经 8.3 短路径等形态启动时 `current_exe`（integration.rs:551）写入的 Run 值前缀不匹配：升级后自启动仍指向旧版本目录；卸载后 Run 项残留成孤儿项（登录报找不到文件）。最小修复：匹配时对两边做短路径归一（`GetShortPathName` 比对），或卸载时对以 `{app}` 长短任一形态开头的值都删除。
6. **`Local\BeeK-Running` 会话局部性缺口** — beek.iss:740、793。触发：同一用户快速用户切换的两个会话，会话 A 卸载时会话 B 的实例不持同名 `Local\` 互斥体，文件在运行中被删；且 beek.iss:775/802 文案声称 install.lock 能发现“BeeK 正在运行”，实际该锁只能发现安装器。最小修复：互斥体改 `Global\`（配每用户安装语义确认），或至少修正提示文案只归因于安装器。
7. **打包脚本两处 fail-closed 的健壮性缺陷**（不破坏门禁，但 Formal 路径易碎）：(a) package.ps1:639 `/Sbeek="…signtool.exe" sign …` 参数内嵌引号+空格，在 Windows PowerShell 5.1/7.2- 的原生参数传递下会被改写，signtool 路径含空格（默认就在 `Program Files (x86)`）时 ISCC 签名步失败报 throw；(b) package.ps1:632 签名非 Valid 时 `SignerCertificate` 可能为 `$null`，`Set-StrictMode -Version Latest` 下访问 `.Thumbprint` 抛出的是 PS 错误而非预期的清晰报错。另 package.ps1:615 `Capabilities` 先 `ReadToEnd` 再 `WaitForExit` 且 stderr 重定向未排空，stderr 超 ~4KiB 管道缓冲会死锁挂起打包。最小修复：`/S` 参数用 `\"` 转义或经 response 文件；判空后再取 Thumbprint；stderr 异步读或 `RedirectStandardError=$false`。

**不可从附件核验项（不计为发现）**：`util::wide` 是否追加 NUL 终止符（REG_SZ 规范要求，integration.rs:551-553）；应用运行时是否/如何持有 `install.lock`（影响 `--purge-user-data` 子进程与卸载锁的相互作用）。二者均在附件范围外。

**最终裁定：有限 PASS。** 附内代码在四个焦点上的核心机制（双真实槽、总截止、恰好一次终态、SSE 四层上限、精确资产+固定 URL、安装锁时序、Formal 签名门禁）经对抗性推演均成立；上述 Minor 建议修复后无需重新评审架构，但发布决策仍受既有保留项约束，G5 未通过。
