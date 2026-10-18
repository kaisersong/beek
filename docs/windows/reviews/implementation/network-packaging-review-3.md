# 复核结论：有限 PASS（仅限本子系统；G5 仍未通过）

## m6 关闭核验（调用点证据成立）

- `run()` 首条语句即分发 `--purge-user-data`，清理后立即 `return`（失败 `exit(1)`），先于会话检查、`ui::run` 及一切初始化；`install_guard()` 仅在 `ui::run` 内调用，purge 子进程永不取锁，与卸载器独占锁（含 m4 持锁至 `DeinitializeUninstall`）无交集。卸载链路 Exec→Code=0→继续删 Run 项，闭合。
- R2 的 purge 冲突判定确认为**因未附依赖而产生的假设，非真实代码缺陷**：实际调用点将参数分发置于 guard 之前，冲突在真实代码中不存在；卸载器无需提前放锁，m4 关闭状态不受影响。
- guard 语义在真实路径成立：正常启动经 `check_platform`→`install_guard()?`，Err 经 `util::fatal` 退出（安装/卸载期间启动即拒并提示），`Ok(None)` 兼容无 marker 的便携运行。

## R2 遗留 Minor 关闭

`let _installation = install_guard()?` 为具名绑定而非 `_`，按 Rust 语义存活至 `ui::run` 栈帧结束（即 `Application::run` 返回），句柄全程持有，“调用方提前 drop 放锁”的担忧消除。

## 附带核验（未另开支线）

- `--capabilities`/`--verify-pair` 同样在会话检查前分发，CI（session 0）可执行预检；`capabilities` 的 `native_machine`、`test_fixtures` 字段与 package.ps1 的 5 秒预检、34404 原生门禁、fixture 拒绝逻辑逐项对应闭环。
- 会话检查要求 `session != 0`，与交互式桌面前提一致，且不影响已分发的运维子命令。
- 应用运行中手动直跑 `--purge-user-data` 不经 guard，属用户显式破坏性命令，不计为本子系统缺陷。

## 新增实测修复

本附件无新增差异或实测材料，仅调用点证据，无复核对象；如实说明，不作臆测。

## 新引入 Bug

证据代码内未发现可触发问题（分发顺序、错误退出码、绑定生命周期均核对）。

## 裁定

七项问题全部 CLOSED，无未关闭重大问题。对该子系统（附件所及网络池/SSE、更新检查、资产校验、打包与安装器范围）给予**有限 PASS**。保留项不变：未签名预览、缺外部实测仍为发布阻塞，G5 未通过，不可公开发布。