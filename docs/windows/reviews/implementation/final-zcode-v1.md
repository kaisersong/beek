# 最终有限源码共识判定

**判定：有限共识 PASS。** 基于四个子系统评审链（selection R3、storage R2+R4调用点复核、network-packaging R3、UI/native/speech/lifecycle R3→R4→R5）与本附件交叉核对，未发现新的真实缺陷或证据矛盾，不构成 REVISE。

## 标识（引用声明值，非独立重算）

- 清单：`final-source-manifest-v1.json`，SHA256 `584eb1f1...5faadb4`（Codex 本地重算，本方未重算、亦不声称重算）
- 源码指纹：`6110fb94...4b30f68`（与附件头部及 Codex 自审一致）
- 发布构建 ID：`69e09ecb...64d71d49`；基线提交 `09cfe6d`（与会话起点 git 记录吻合）
- 可核内部一致性：`plan_sha256` 与清单内 `docs/windows/windows-port-plan.md` 条目一致；清单覆盖文件集与工作区增改集合（`src/platform/windows/*`、`src/bin/*`、`tests/windows/*`、`scripts/windows/*`、packaging、licenses 等）对应，无缺漏矛盾。

## 交叉核查结果（链路闭环）

1. **selection R3 的边界留观**（`restart_events` 置 Some 后失败路径是否复位）已由 ui-lifecycle R4 实码关闭：UI 在 `RestartEvent::Failed` 分支显式置 `restart_events=None`，Busy 由 Drop 复位，重试通路成立。
2. **storage R2 的 N1/N2**：N1（空白 legacy key 迁移报错）经 R4 `startup_settings` 的 trim 判空短路关闭；N2（冷启动超父侧预算）经父侧 `pipe.connect(30_000)` 放宽与调用点复核关闭，与附件“30秒GPU冷启动边界已复核”声明吻合。
3. **三次假设撤回均有记录支撑**：后台候选无限等待（selection R3 设计性关闭）、卸载 purge 锁冲突（分发先于 guard，真实代码无冲突）、父侧 PID 清理（父进程持有 Child::drop+CAS）——撤回理由都来自补充实码，非口头撤回。
4. **R4→R5**：唯一遗留 Minor（keep_accessible 延迟尺寸同步）已在 R5 关闭，激活/最小化语义保留，无回归。
5. **末批新增仅为测试面**：产品 runner 会话 0 拒绝、新证据目录强制、5 秒预检、cwd 无关 pair 校验，均不触产品运行契约；Explorer 守卫拒绝实机重启测试并保持待测，是正确的守卫行为而非缺陷。Codex 自述的 23/19/22 项实测计数本方无附件可证，仅作其自述记录，不影响源码共识。

## 限制声明

本共识仅覆盖各评审会话实际附码范围与上述一致性核对；不声称独立重算任何 hash，不声称对未附全仓逐行独立审阅。未附依赖契约（`process::Jobs`、`util::Handle` 等）维持各轮假设记录，若变动需回溯。

## 剩余环境验收（发布阻塞，非源码问题）

签名（当前未签名预览）、多显示器/热插拔、干净机、第二 GPU、8 小时浸泡、完整 L1/故障矩阵、macOS 完整运行回归、以最新构建重跑的最终 release/安装卸载/空闲回归、Explorer 实机重启测试；G6 自动模拟复制与 G7 自动安装更新未启用。

**结论：源码范围有限共识 PASS；G0–G5 未全通过，不请求亦不构成发布批准。**