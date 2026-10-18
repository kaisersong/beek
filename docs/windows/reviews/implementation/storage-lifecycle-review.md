Session: sess_72889fb4-eecd-4f72-877a-333c3b8d7e03

# 独立对抗性评审：storage-lifecycle 子系统（附件快照）

审查范围仅限附件四个文件（`src/core/settings.rs`、`src/platform/windows/store.rs`、`src/platform/windows/lifecycle.rs`、`src/platform/windows/pair.rs`）。`util.rs`（`Handle`/`nonce`/`wide`/`known_folder`）与 UI/调用方代码未随附件提供，涉及处已在下文标注假设。结论先行：**REVISE**，依据是下面唯一一个 Blocker 加两个 Major。

---

## Blocker

### B1. 旧版明文 API Key 迁移链路断裂：读入后既不可用，首次 Keep 保存即被静默销毁

**位置**：`src/platform/windows/store.rs:565-601`（`parse`）、`src/core/settings.rs:87-133`（`transact`，关键行 120）、`src/platform/windows/store.rs:450-460`（`Store::key`）

**触发路径**：
1. 旧版配置 `{"providers":{"ark":{"api_key":"K","model":...}}}` 存在。`parse()` 把 `K` 装进 `cfg.provider.api_key`，`credential_ref` 为 `None`。
2. 迁移后的进程内，`Store::key()` 只认 `credential_ref` 和 `BEEK_ARK_KEY` 环境变量，**完全不读 `provider.api_key`**——即使不保存任何东西，升级后第一次取词翻译就报“请在设置中填写 API Key”，尽管 Key 明明还在磁盘上。
3. 用户随后在设置里改任意无关项（如自动启动）并保存，走 `KeyEdit::Keep`：`transact` 第 120 行 `next.provider.api_key.clear()`，提交的配置 `credential_ref: None` 且 `api_key` 被 `skip_serializing`——**明文 Key 从磁盘消失，且从未进入凭据管理器**。无任何迁移完成。

附件内不存在任何把 `provider.api_key` 送入 vault 的代码路径（`pub` 字段理论上允许未见到的 UI 层自行发 `Replace` 补救，但即便如此 `Keep` 分支的销毁语义仍在契约内）。这同时命中“凭据提交失败”与“旧配置迁移”两个评审焦点：迁移被声明（注释 "Read legacy plaintext once for migration"）但从未闭环。

**最小修复**（在 `transact` 入口把 Keep 升级为迁移性 Replace，不依赖调用方自觉）：
```rust
let edit = match edit {
    KeyEdit::Keep if old.provider.credential_ref.is_none()
        && !old.provider.api_key.is_empty() =>
        KeyEdit::Replace(old.provider.api_key.trim().to_owned()),
    e => e,
};
```
这样复用现有的 create→复核→commit→回滚原子序列，vault 保持唯一真源。

---

## Major

### M1. 重启握手的授权窗口不对称：子进程等提交消息只给 1 秒，父进程等 UI 授权给 5 秒

**位置**：`src/platform/windows/lifecycle.rs:885`（`Pipe::read` 写死 `complete(..., 1000)`）、`1159-1162`（父侧 `grant_rx.recv_timeout(Duration::from_secs(5))`）、`1275`（`child_preflight` 中 `let response = pipe.read()?`）

**触发路径**：用户触发重启 → 父进程线程收到 `ChildReady` 后最多等 **5 秒** UI 调 `grant_restart()`（保存状态、动画、消息循环繁忙时完全可能超过 1 秒）才写 `ParentExitCommitted`；而子进程在 `child_preflight` 里用同一个固定 **1 秒** 超时的 `pipe.read()` 等这条消息。UI 授权耗时落在 1–5 秒之间时：子进程读超时 → `child_preflight` 返回 Err → 子进程退出 → 父进程随后 `pipe.write` 失败 → kill（已死）→ 报 `Failed`。**应用各部分均健康，重启却确定性假性失败**。

**最小修复**：给 `Pipe::read` 加超时参数（如 `fn read_with_timeout<T>(&self, t: u32)`），`child_preflight` 等待 `ParentExitCommitted` 处改用 ≥6000ms（父侧 5s 授权 + 写余量），其余调用点维持 1000ms。

### M2. `validate` 不限制 `base_url`/`hotkey` 长度，可写出重启后应用内永久无法修复的配置

**位置**：`src/core/settings.rs:56-74`（`validate` 只限制 `model ≤ 1024`）、`src/platform/windows/store.rs:259-261`（`load` 的 1MB 检查提前 `return Err`）

**触发路径**：设置界面粘贴超长 `base_url`（如 ~1MB，`reqwest::Url::parse` 不设长度上限，`validate` 通过）→ `commit` 成功写出 >1MB 的 `config.json`（写出端无大小检查）→ 重启后 `load()` 在 metadata 检查处直接报错返回，**既不走 `preserve_invalid` 备份，也不设置 `expected`** → `expected` 保持 `None`，此后一切 `save` 被 `commit` 的乐观哈希检查拒绝（“配置已被其他会话修改或尚不可读”）→ 只能手工删文件。不变式被破坏：validate/commit 接受的配置，load 却永久拒绝且应用内无恢复路径。`hotkey` 完全未校验，同病。

**最小修复**：`validate` 补 `base_url.len() > 2048`、`hotkey` 长度 > 64 的拒绝（可选加固：`load` 的超限分支改走 `preserve_invalid`，使应用内覆盖保存可行）。

---

## Minor

### m1. 损坏备份文件永不过期清理，反复启动无限累积

**位置**：`src/platform/windows/store.rs:430-442`（`cleanup_older_than` 的文件名过滤只匹配 `config.<48hex>.tmp`）；备份名产生于 `270-282`（`config.corrupt-<nonce>.json`）

**触发路径**：配置持续损坏（用户未处理）+ 自动启动，每次启动 `load` 失败都 `fs::copy` 一份新的 ≤1MB 的 `config.corrupt-*.json`，后缀是 `.json` 不匹配 `.tmp` 过滤，永不清理。**修复**：过滤条件加入 `config.corrupt-*.json`，沿用同一 24h 年龄阈值。

### m2. 监听线程静默退化：`Pipe::server(...).ok()` 吞掉持续性创建失败

**位置**：`src/platform/windows/lifecycle.rs:1069`（listener 循环 `next.take().or_else(|| Pipe::server(...).ok())`）

**触发路径**：主管道名被同用户进程长期占用（`FILE_FLAG_FIRST_PIPE_INSTANCE` 使我方创建持续 `ERROR_ACCESS_DENIED`）→ 监听线程无限 20ms 空转、无任何错误上报 → 主实例正常显示但激活功能静默失效，二次启动 1 秒后报“已在启动或暂时无响应”，问题无处可见。**修复**：对连续失败计数（如 50 次）后向 `events` 上报错误或记日志。

---

## 已对抗性核查、未发现问题的路径（供对账）

- **凭据失败回滚与崩溃孤儿**：`Replace` 的 create→读回复核→commit→失败删 staged 保 old（settings.rs:103-126，含测试）；commit 成功后旧密删除失败或中途崩溃留下的孤儿，由 `cleanup_older_than(24h)` 回收。跨会话竞态下 24h 年龄下限同时保护了对方会话刚 staged/刚提交的凭据（store.rs:403-414），方向性正确。
- **配置原子性与乐观并发**：临时文件 + `ReplaceFileW`/`MoveFileExW`、崩溃遗留 `.tmp` 有清理；`expected` 哈希比对使“读到旧配置后他人已提交”的保存被拒绝而非覆盖（store.rs:519-521）。锁序（`self.lock`/`file_lock` 反序）被 file_lock 的 300ms 超时兜住，只产生瞬态报错不致死锁。
- **单实例仲裁**：会话级 mutex + `Identity{pid, created, session}` 绑定进程创建时间，防 PID 复用冒充；`FIRST_PIPE_INSTANCE` + 双向 peer 核验失败即闭合；所有者先写文件后建管道的窗口由 1 秒重试覆盖；重启后子进程仅在 `WaitForSingleObject(父进程, 10s)` 确认死亡后才接管，无双实例并存路径。
- **IPC 长度**：双向 4096 上限、Owner 文件 ≤4096、vault blob 写 ≤2560/读 ≤8192，均闭合。

**未评审依赖**：`util.rs` 的 `nonce()` 熵源与 `Handle` 的 CloseHandle 语义、UI 层对 `KeyEdit` 的选择逻辑不在附件内；B1 的最终定级以这两处不存在隐藏补救为前提，即便存在，`transact` 侧加固仍是应做的最小修复。

---

## 结论：**REVISE**

Blocker B1（迁移链路断裂导致用户凭据静默丢失）必须先修复并补一条“旧配置 + Keep 保存”的回归测试；M1/M2 建议同批处理。此结论**仅针对本 storage-lifecycle 子系统**，不构成 G5 通过的判断，也不构成可公开发布的判断。
