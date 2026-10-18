# 复审报告（Revision 3 · 终验）

**结论：该子系统有限 PASS。** 未发现新的可触发缺陷；R2 遗留 Minor 全部核实修复；此前基于“依赖未附”的两项假设，一项经实际架构澄清而消解，一项确认仍在本子系统边界外。

## 假设与真实缺陷的区分

**R2-m1（陈旧 helper PID）——假设消解。** 我当时假设 `Child::drop` 在一次性 helper 进程内，崩溃即无法清理。实际实现：PID 原子量与 `Child` 均在父进程，`reaped()` 由父进程 10ms 循环驱动（`exited()` 且 reader 线程已结束才 join），异常 helper 死亡同样触发父侧 Drop 的 `compare_exchange(self.pid,0)` 清理；CAS 仅在 `query.clipboard` 时执行且比较旧值，不会误清后继 helper 的新 PID。残余暴露仅为 helper 退出到下一次 reap 的 <10ms 窗口且需 copy_to_popup 开启，可忽略。CLOSED。

**lifecycle 子进程静默退出（R2 关注项）——仍为不可核实假设。** `if !restarting` 分支的父侧行为依赖未附的 lifecycle 实现，本附件未提供新证据。按约定不计为代码缺陷，保留为跨子系统留观项，不阻塞本结论。

**CloseClipboard 推进 sequence——真实缺陷，修复核实。** 这是实测发现的 Windows 行为：写入方先前确认的是 `SetClipboardData` 后的中间值，链条重绑会用旧 sequence，后续校验将误报 SourceChanged。现写入尾段在 `drop(_clipboard)`（显式 CloseClipboard）之后取最终 `after`，并要求 owner 稳定且双读 `GetClipboardSequenceNumber()` 一致才返回；任何竞争写入（owner 变化或 sequence 再推进）返回 `SourceChanged`，fail-closed 方向正确——此时剪贴板内容已非本次写入，报失败语义准确。配合 session1 实测（双/三连粘贴、紧接 Cut/Paste、真实竞争进程持锁下字段文本与原剪贴板均保全），M2 类链条与写入竞态均有实证。测试方法修正（先种 DISTINCT 哨兵再回读断言、旧弱断言显式标记 inconclusive 并保留）消除“旧剪贴板内容假通过空断言”的假阳性，采信。

## R2 遗留修复核实

- 编辑阻塞保存：状态区分“请等待剪贴板编辑完成后再保存”/“设置操作繁忙”，CLOSED。
- 派发清空分支：措辞改“设置窗口已不可用”，并同时写 `last_settings_status` 与存活 form 的实时 status，CLOSED。
- manual Fault：`Outcome::Fault` 现与 Empty 同构——`Preview|Clipboard(false)` 写入可见反馈、form 关闭时重开窗口承载消息（`last_settings_status` 兜底，`unwrap` 前必有赋值，安全），CLOSED。
- 声明的“Settings/快照窗口激活前钳制工作区”无代码佐证，但不对应任何 OPEN 项，不影响结论。

其余所示区域（Edit 重绑、Saved 签名门、Request 三重门、有界队列）与 R2 已审语义一致，无回归。

**放行状态**：本子系统有限 PASS；G5 仍未通过，不得公开发布；lifecycle 留观项移交对应子系统评审。