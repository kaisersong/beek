Session: sess_70ea62df-888f-4da5-8814-eb8586320616

评审完成。已通读附件全部 5 个文件（`protocol.rs`、`policy.rs`、`process.rs`、`selection.rs`、`helper.rs`、`util.rs`），仅做静态对抗审查，未访问仓库其他文件、未修改任何内容。**结论：REVISE**——无 Blocker，有 1 个 Major 必须修，4 个 Minor 建议修。

## Major

**M1：探针被“静默终止”吞掉后，该应用被永久拉黑（无自愈）。**
`policy.rs::Health::begin` 在第 4 次尝试前把 `probe_used` 置位；`allowed()` 一旦看到 `failures>=3 && probe_used` 就永久拒绝。但 `selection.rs` 协调线程里存在多条**不产生任何 outcome、也不调用 `health.failure/success`** 的杀槽路径：SYSTEM_EPOCH 变化时的 `slot.kill()`、`health.paused()` 时的 kill、容量满时的 stale-kill（`min_by_key(oldest)` 会杀掉正忙着探测别家源的槽）、`identity_alive` 失败时的 kill。触发链：某应用因 UIA 慢/挂累计 3 次失败（1s/5s/30s 退避都走完）→ 30 秒后唯一一次探针在 1.5s 窗口内被一次 alt-tab（epoch 变化）或容量抢占静默杀掉 → `probe_used` 已置位且永远等不到 outcome → 该 (pid, created) 之后所有取词请求在 waiting 过期检查处直接变 `Empty(SourceChanged)`，直到该进程退出被 `retain_live` 清理或用户手动 `restore()`。应用本身完全健康，属于用户可见的永久性功能丧失。

最小修复（只改 `policy.rs`，把“一次性探针”改成“每 30 秒至多一次的探针”，被遗弃后可自愈）：
```rust
// Health::begin 内：
if f.failures >= 3 { f.next = now + 30_000; }   // 原为 f.probe_used = true
// allowed() 删去 && !(f.failures >= 3 && f.probe_used)，并删除该字段
```
现有测试 `exactly_one_probe_and_restore_cannot_bypass_spawn_budget` 的全部断言在此修复下仍然成立（探针失败→failures=4→quarantine 不变；30 秒窗口内不可重复探针不变），无需改测试。

## Minor

**m1：剪贴板读取 16,385–约 32,769 字符被误报为 Empty。** `helper.rs::clipboard` 的尺寸上限 `(MAX_CHARS*2+2)*2` 字节放行了超过 `MAX_CHARS` 字符的文本，随后 `!selection.validate()`（字符数超限）被映射成 `Status::Empty`；协调器把 Empty 当成功（`health.success` + 弹“无选区”）。修复：validate 失败（或前置 `text.chars().count() > MAX_CHARS`）时返回 `Status::Unsupported`。

**m2：被管理性杀掉的在途查询 id 没有终态 outcome。** 与 superseded/pending_expired/worker_* 都会发 Fault 不同，上列静默 kill 路径对 busy 槽的在途 id 不发任何 `Outcome`。本快照不含消费端（UI 另行评审），无法证实会泄漏，但契约不一致；建议这些路径补发 `Outcome::Fault(id, code)`，并在 UI 评审时核对该 id 是否有兜底超时。

**m3：超长 UIA 选区返回 `Failed`，白白回收健康 worker，且截断检查是死代码。** `helper.rs::selected` 里 `raw.len() > MAX_CHARS*2+1` 永远为假（`GetText(maxLength)` 本身封顶），真正兜底的是 `chars().count() > MAX_CHARS → Err(Failed)`；协调器对 Failed 会 kill+重拉进程。改为返回 `Unsupported`（保留 worker）并删掉死检查。

**m4：`hStdError` 并入协议管道。** `process.rs::Child::spawn` 把 `hStdError` 设为 `stdout_child`，任何杂散 stderr 写入（如 panic 消息）都会污染 JSON 行协议并记为源归因的 protocol fault。建议 stderr 指向 NUL。

## 已核过且成立的正确性（不复述代码，只列结论）

原子作业归属（JOB_LIST 双作业、CREATE_SUSPENDED + 取消复查、KILL_ON_JOB_CLOSE 覆盖主进程崩溃）、6 次/10 秒共享 spawn 预算与 2+1 槽位上限自洽（含 Starting/terminating 计入）、父进程与 helper 两侧的单帧上限/严格 schema/nonce/build/id/source/epoch 校验、剪贴板读写的前后双检（sequence+owner）防 TOCTOU、事件回调零 UIA 调用 + 打包看门狗、helper 心跳 500ms 对 1500ms 看门狗、父子管道串行性（不存在 write_all 与 sync_channel 互堵）、句柄与 SAFEARRAY/HGLOBAL 的 RAII、`process_alive` 的 pid+created 复用防护——均未发现可触发缺陷。

按约定：G5 未过、签名/外部实测缺口不当作源码问题，本结论仅针对该子系统的源码正确性，不构成可发布判断。修复 M1（建议连 m1–m4）后此子系统可复审转 PASS。
