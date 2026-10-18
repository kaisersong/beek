Session: sess_de3c3049-604c-435c-b808-7a0e202fec62

# 复审报告（Revision 2）

**结论：该子系统有限 PASS。** 首轮 3 Major、8 Minor 全部核实 CLOSED，未发现修复引入的 Blocker/Major；新发现 3 个 Minor（不影响放行）。G5 仍未通过，本结论不构成公开发布签核，范围仍限附件三文件差异。

## 首轮问题关闭核实

- **M1 CLOSED**：`native.rs::procedure` WM_CLIPBOARDUPDATE 现取 `GetClipboardOwner` 的 PID 与 `CLIPBOARD_HELPER_PID` 比对，helper PID 在 ResumeThread 前发布、Drop 时 CAS 清除；owner 查询失败（pid==0）时放行，fail-open 方向正确，不会吞外部复制。回声路径（复制译文→自动剪贴板蜜蜂）被切断。
- **M2 CLOSED**：`paste_value` 改返回 `Option<新revision>`，解析臂统一经 `last_applied_edit` 对同字段队列中 `ticket==旧值` 的 Paste 重绑（含提交后同帧到达的 intent，由 `Command::Edit` 端 `at<=completed` 补绑）。核查了连续双击 Ctrl+V、三连 paste、paste 与 cut 交错、中途输入使 revision 变化等链路，重绑链均正确，不再出现“第二次粘贴静默丢弃却报成功”。
- **M3 CLOSED**：`Empty`/`Fault` 臂对 `Purpose::Copy` 均写入面板重试文案；`Command::Copy` 无来源时同样反馈。
- **m1 CLOSED**（Verify 不匹配 else 分支隐藏蜜蜂）；**m2 CLOSED**（派发无来源时 `edits.clear()` 并留 `last_settings_status`）；**m3 CLOSED**（`applied` 区分“已完成/已复制未剪切/未完成”，cut-Copied-未应用不再触发 failed_copy 误清粘贴队列）；**m4 CLOSED**（系统 epoch 复位写 form 状态）；**m5 CLOSED**（`last_settings_status` 种子重开窗口；`save_revision` 签名防止异步 Save 覆盖提交后新输入、条件化清 key 字段与 clear 标志，逻辑核实无误）；**m6 CLOSED**（`range_from_utf16` 委托带钳制的 `text_model::utf16_range`）；**m7 CLOSED**（`paste_value`/`cut_complete` 拒绝 `marked_range` 激活期间应用）；**m8 CLOSED**（三处 open_* 返回 Option + diagnostics，仅设置/宿主窗口失败 fatal，buildid 改 `.get(..12)`）。

## 修复引入的新问题（均 Minor）

1. **陈旧 helper PID 误吞外部复制**（`native.rs` M1 残留）：helper 异常终止未走 Drop 时 `CLIPBOARD_HELPER_PID` 残留，Windows PID 复用后，被复用进程的复制在开启 copy_to_popup 时被永久静默忽略。最小修复：PID 命中时再校验进程创建时间或映像路径，不符则清零原子量并放行。
2. **派发清空分支的文案与反馈**（`ui.rs` edit 派发）：`source=None` 也可能只是设置窗口瞬时非前台，却统一记“旧设置窗口已关闭”，且窗口仍打开时 live status 不更新，用户无感知；`Command::Save` 被编辑队列拒绝时复用“设置操作繁忙”同样词不达意。最小修复：按窗口句柄死活区分文案，并在 form 存活时同步写 `f.status`。
3. **manual 请求 Fault 无反馈**（`ui.rs` `Outcome::Fault` 臂）：Empty 分支为 `Preview|Clipboard(false)` 补了 `selection_status` 反馈，Fault 分支没有——托盘手动读取若以 Fault 失败仍只隐藏蜜蜂。最小修复：Fault 臂复用同一 manual 分支。

**关注项（不计 bug，lifecycle 未附无法核实）**：子进程启动失败改为 `if !restarting` 才 fatal，restart child 在 ChildReady 前失败将静默退出；若父进程缺乏子进程死亡探测，重启可能无提示停滞，请 lifecycle 侧确认。

## 复审确认

有界性无回归（通道 32/32/2/1、edits≤4、单槽 pending/mailbox/edit_active/last_applied_edit/save_revision）；epoch/点击复核/旧响应隔离与快照冻结语义未动且仍成立；live 无激活（NOACTIVATE+MA_NOACTIVATE+focus:false）与热键激活快照面板（Enter/Esc/Ctrl-C）行为未变；IME 守卫（is_window_active、marked_range、UTF16 钳制）较首轮更严。修复 M1 残留与三个新 Minor 后可直接进入下轮；本轮子系统**有限 PASS**。
