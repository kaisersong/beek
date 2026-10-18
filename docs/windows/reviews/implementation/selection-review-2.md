Session: sess_70ea62df-888f-4da5-8814-eb8586320616

复审完成（附件 695 行已完整读取，未动其他资料）。**结论：该子系统有限 PASS**——首轮 5 个问题全部核实 CLOSED；修复新引入 2 个 Minor（不阻塞），另有边界说明。

## 首轮问题关闭核实

**M1 CLOSED。** `Health::abandon` 仅在 `failures==3 && !quarantined && probe_used` 时释放探针并冷却 1 秒：被管理性取消的探针可重试（有界，仍受 6 次/10 秒 spawn 预算与 1 秒冷却约束，且需用户动作驱动，无自旋），真实失败仍走 `failure()` 第 4 次即隔离。全部遗弃路径已接通：epoch 变化、paused、stale-kill、容量抢占、`source_gone`（经 `cancel_worker/cancel_slot`）、启动失败（`failure()` 的 `!clipboard && !handshaken` 分支与 `launch` Err 分支）、以及回复态 Busy/SourceChanged/Failed 的 else 分支。测试逻辑与实现一致；真实协调器 fixture（三超时→30 秒探针被 epoch 取消→第五次 Selected）佐证。

**m1 CLOSED。** 剪贴板路径：尺寸超限→TooLong、字符数超限在 validate 前显式检查→TooLong；剩余 validate 失败仅剩空文本→Empty，语义正确。**m3 CLOSED。** UIA 路径两处改 TooLong；协调器把 TooLong 计入 success 集合、不 kill worker；原死检查 `raw.len() > MAX_CHARS*2` 现可被 GetText 截断哨兵长度 32769 触发，有效。**m2 CLOSED。** 五条静默杀路径现对在途 id 均发 Fault（epoch 连 waiting 与 pending 都补发 `system_context_changed`），idle 槽不误发，terminating 守卫防双发。**m4 CLOSED。** stderr 指向可继承 NUL 句柄并纳入白名单，不再混入协议管道；失败路径由 Registration Drop 的 CAS 清理 `CLIPBOARD_HELPER_PID`，无泄漏、无双清。

## 修复新引入问题（Minor）

**n1：`RestartChild::drop` 用 `WaitForSingleObject(process, INFINITE)`。** TerminateJobObject 后被内核卡住的线程可使候选进程迟迟不退出，Drop 所在线程（大概率退出/更新路径）无限阻塞，违背本子系统处处有界的纪律。最小修复：有界等待（如 5 秒）后直接返回——进程已被终止，关闭句柄不会复活它。

**n2：`restart_job()` 重复设限。** `Jobs::new(1)` 先设 256MB 内存上限，随后整体覆写为 KILL+ACTIVE(1) 把内存上限抹掉。终态正确（整实例本就不该受 helper 级内存帽），但双重设置易误读，建议直接以终态标志构造并注释。

## RestartChild 与原 UIA 上下限核对（按附件要求）

独立作业、独立 `Jobs::new(1)`，不进 coordinator 的 main_job（上限 3）与槽位/Health 预算，2 UIA+1 剪贴板上限不受影响；提交前 KILL_ON_JOB_CLOSE 保证崩溃不产生孤儿候选，`commit()` 覆写为仅 SILENT_BREAKAWAY_OK 后交接成立，残留 job 无任何限制标志、无害。

## 边界与非缺陷说明

`CLIPBOARD_HELPER_PID` 的读取方在 native.rs（未附），写入侧已核稳妥（cap 1 保证无并发注册、CAS 防误清、陈旧 pid 窗口仅数十毫秒）；新旧行实例交接期的短暂双 coordinator 重叠属更新流程评审范围；ElementFromPoint 惰性化未改变候选优先级语义。

**有限 PASS 范围**：本快照内 selection 子系统的 UIA/进程/协议/退避正确性。G5 未过，不可据此声称可公开发布；n1/n2 建议随下轮一并处理。
