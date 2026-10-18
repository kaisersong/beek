# R4 增量复审（最终意见）

**结论：本增量有限 PASS。** 新增实现未发现 Blocker/Major 缺陷；此前跨子系统留观项（restart 子进程静默退出）经 lifecycle 实码核实正式关闭；遗留 1 个 Minor（同缺陷类未覆盖到 keep_accessible 路径）与若干未附依赖假设，逐项列后。G5 仍未通过，不构成公开发布批准。

## 留观项关闭核实（lifecycle.rs 实码）

父侧 restart 线程对每条子进程失败路径都有界：创建等待 5s、管道连接 30s、提交回读 1s；子侧 `child_preflight` 提交等待 6s、父进程退出等待 10s。子进程在任何阶段死亡，父侧要么读管道失败要么超时，最终必达 `RestartEvent::Failed` → UI 置 `restart_events=None` 并在 form 显示错误，旧实例继续运行——"失败子不弹独立 fatal"因此安全，无僵尸等待。`commit_exit` 仅在 Committed 后调用，使 `Instance::drop` 不击杀已接管子进程；未提交则 cancel+job.kill 防孤儿。另核实崩溃恢复：进程死亡即关闭全部句柄、命名互斥体随之销毁，`CreateMutexW` 对已存在互斥体忽略 bInitialOwner，故崩溃后重开不会永久命中"已在运行"分支（该分支仅在前实例真实存活时进入）。`Event::IpcUnavailable` 也有了真实生产者（listener 连续 50 次建管道失败），R2 的死代码疑问一并消除。

## 新增代码核查

**1. WM_APP+23 延迟尺寸重投——设计正确，存在一处未覆盖。** 消息经 PostMessage 在 UI 线程借用结束后处理，规避 GPUI Window 双重借用；handler 内 `GetClientRect` 取当下物理尺寸（DPI 变化自校正），`WM_SIZE` 的 LOWORD/HIWORD 打包正确、clamp 防越界，经 `DefSubclassProc` 转交原窗口过程无指针生命周期问题，WM_NCDESTROY 反注册、消息号与 TRAY/CONFIG 无冲突、重复 show_popup 重投幂等。实测按钮可见，采信。
**缺口（Minor）**：`open_form`/快照 `open_panel` 的 `keep_accessible` 同样在 `w.update()` 借用内做含尺寸的 SetWindowPos（工作区小于窗口时收缩，如 1366x768 笔记本对 720x780 逻辑的设置窗），却不在这两个窗口装子类、也不投递同步消息——同缺陷类未覆盖。设置窗有 `overflow_y_scroll` 兜底可滚到保存键，快照 420 逻辑高度极少超工作区，故降为 Minor。最小修复：为这两类窗口复用同一子类+WM_APP+23，或 keep_accessible 仅钳位不缩尺寸。建议补一次实机验证借用失败是否在该路径复现。

**2. speech.rs——通过。** 固定 STA 线程、首次请求才 CoInitializeEx（避免消息泵前 COM 阻塞）、Speak 全程在该线程；输入通道容量 1、结果通道容量 2 均有界，`try_send` 失败即"朗读繁忙"，UI 每tick `poll` 且 `g==generation` 才应用——过时回执安全丢弃；不 join 线程，退出由 `exit_deadline` 兜底。仅一 cosmetic：结果通道满 2 时第三条回执丢弃，状态停在"正在朗读…"，不修可接受。

**3. 实时卡前台持续核对——通过，附带效应与设计一致。** 每 tick 校验 `!snapshot && !is_current(source)` 即取消网络并隐藏，补上 SKIPOWNPROCESS 漏掉自家 Settings 激活的缺口；快照豁免、拖拽/按钮不夺焦故不误杀。注意托盘右键菜单（`SetForegroundWindow` 到隐藏 owner）也会按同一规则关闭实时卡——属"实时卡跟随前台"设计内行为，记录非缺陷。

**4. 钳制与 phase 日志——通过。** `startup_settings` 顺带把纯空白 key 归零；`last_settings_status` 以 initial_error 种子化，--autostart 场景首次打开设置也能看到启动错误；phase 日志仅结构性字段，无配置/正文泄漏。

## 假设清单（未附依赖，未据此定性缺陷）

`process::RestartChild/Jobs`（commit/kill 语义）、`util::Handle` 关闭即释放、`util::is_current/work_area/current`、GPUI 对合成 WM_SIZE 的处理（已有实测佐证）。这些契约若变化需回溯上述结论。

## 最终判定

四项增量 + 留观关闭全部核实，仅余 1 个 Minor（keep_accessible 缩尺寸路径的延迟同步缺失，建议下批随实测一并修复）。源码层面**有限 PASS**；环境验收（多显示器、浸泡、签名）仍属 G5 范畴未完成，**不得公开发布**。