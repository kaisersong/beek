Session: sess_72889fb4-eecd-4f72-877a-333c3b8d7e03

# 复审结论：本子系统**有限 PASS**（G5 未通过，不可公开发布）

首轮五个问题全部核销，修复自查发现两个 Minor 新问题（不阻塞）。逐项依据如下。

## 首轮问题核实

- **Storage.B1 → CLOSED**：`transact` 入口把“无 ref 且 api_key 非空”的 Keep 升级为 Replace，复用既有 create→复核→commit→失败删 staged 保原 key 序列；`startup_settings` 启动即迁移落 ref，`Store::key` 不再出现“有明文却取不到”的常态。回归测试覆盖 commit 失败保明文/成功迁移/序列化不含明文。遗留边缘见 N1。
- **M1 → CLOSED**：`child_preflight` 等 `ParentExitCommitted` 改 `read_for(6000)`；父侧 grant 窗 5s、父读 ChildReady 的 δ<1s，6s > δ+5s 成立。公共 IPC 每操作仍 1000ms，未误伤。
- **M2 → CLOSED**：validate 补 base_url≤4096、hotkey≤64、credential_ref 非空≤128、api_key≤2560，叠加 model≤1024——任何可提交配置远低于 load 的 1MiB 拒绝线，“写出不可再读且应用内无法修复”的不变式恢复。
- **m1 → CLOSED**：备份名改内容散列（64hex）去重，同内容反复启动零拷贝；prune 保留最新 3 份、其余满 24h 删除，且兼容旧 48hex 命名。
- **m2 → CLOSED**：连续 50 次建管失败发诊断事件 + `Event::IpcUnavailable` + 降频 1s；成功即复位，再故障可再次触发。

## 修复自查新发现（Minor）

- **N1**：legacy api_key 为纯空白（或超 2560）时，迁移路径在 trim 后判空/超长报错——启动路径整份配置落到默认值+错误横幅；会话内 Keep 保存被拒。可恢复（重填或 Clear）。建议 `startup_settings` 对 trim 后为空的 api_key 直接返回 cfg 不迁移。
- **N2**：ChildReady 现排在 GPU 初始化与多次配置读取之后，父侧预算仅 connect(5s)+read(1s)；冷启动（杀软扫描/着色器编译）超约 6s 会使重启假性失败（旧实例续跑、可重试）。属有意换取“子进程验证可用后才提交”，建议放宽父侧 connect 预算。

## 新增交接逻辑核查（未发现问题）

Job 原子归属 + KILL_ON_JOB_CLOSE 防父崩溃孤儿；挂起态取消走 Drop 的 job.kill+等待，无挂起孤儿残留；`commit()` 清 KILL_ON_JOB_CLOSE 仅发生在收到 ChildCommitAccepted 之后，此后父崩溃的幸存者恰是已获授权的候选；用户 Quit（未消费 Committed）用显式 TerminateJobObject 杀候选，不依赖该标志；Busy 租约由重启线程+创建线程双持有，创建返回前不可重试；子进程 `bInheritHandles=false`，不污染会话锁；子进程确认父死亡后重读已提交配置，预检快照不一致已消除。

**未附依赖（存疑记录，非本轮问题）**：`Jobs` 包装的 kill/句柄语义、`diagnostics::event`、`Event::IpcUnavailable` 的 UI 处理、Root 对 `restart_events` 的复位。

此 PASS 仅限 storage-lifecycle 子系统本轮修复面；G5 整体未通过，不构成公开发布判断。
