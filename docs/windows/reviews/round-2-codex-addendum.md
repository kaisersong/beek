# Codex v2 追加反例

- 快照 SHA-256：`3677663c2fa62ea9770d6bf0f3aa15ac730387a6335bb9f5a805ad1314675048`。
- 最新 Verdict：REVISE。此追加意见覆盖 `round-2-codex.md` 的 PASS；此前六项关闭仍有效。
- 本反例在收到 xiaok/ZCode 最终意见前记录，不引用或查看它们的内部思考内容。

## CODEX-R2-01 — Major：凭据和配置是两个存储，缺少跨存储提交顺序

§8 要求凭据先写 Credential Manager、配置再原子替换，但没有说明更新旧凭据时是否使用新 credential_ref。如果覆写稳定 target 的旧 key，然后 config.json 更新 model/base_url 时失败，旧配置将配上新 key，原本可用的组合被破坏；config.json 的原子写不提供 Credential Manager 的事务。

最小修正：以不可变、带版本的新 credential_ref 暂存新凭据；读回确认成功后，在一次配置原子替换中提交 provider/model/base_url/ref。配置提交失败时保留旧引用和旧凭据，清理或记录新孤儿凭据；提交成功后才尝试删除旧引用。崩溃恢复以配置当前引用为真相，不在恢复时覆盖已提交的新凭据。

关闭标准：凭据写失败、写后读回失败、配置替换失败、提交前后杀进程、旧凭据删除失败五类注入中，至少有一套已提交的完整配置/凭据组合可用；清理失败不使已提交配置失效。读取失败时不无声切到另一个账户。

## CODEX-R2-02 — Major：G0 Windows 原型需要明确独立构建入口

G0 在 G1 平台拆分之前。如果“最小编译”仍调用当前根 crate 的 main.rs，会先遇到未隔离的 macOS 模块和依赖，从而要求 G1 先完成，任务依赖出现循环。

最小修正：明确 G0 使用独立的 `tools/windows-probe` 最小 crate，仅引入锁定 GPUI、Win32 和必要资源，不导入 BeeK 当前 main/macOS 模块；根应用的平台拆分和真正 Windows 应用构建保留在 G1。G0 结束要把成功参数、最小补丁和原型证据移交 G1，不能拿 probe 编译成功冒称完整 BeeK 已支持 Windows。

关闭标准：全新 Windows 环境可单独构建原型，命令无 macOS 依赖；G1 才执行完整应用构建，二者产物及证据明确区分。
