Session: sess_4a763d88-ca54-4e3a-a975-54944c197eea

# 复审结论：REVISE（仅一项重大未关闭，其余全部 CLOSED）

附件已完整读取（337 行，无截断）。逐项核实如下。

## 已关闭（7 项中 6 项）

- **m1 CLOSED**：`take(FRAME+3)`+读后 `line.len() > FRAME` 判定，使“内容恰 64KiB+CRLF”通过、内容 64KiB+1 被拒，边界语义与文案一致；行缓冲仍有界（≤64KiB+3）；新单测覆盖两种行尾及超限一字节。未引入新问题。
- **m2 CLOSED**：结果改写进 `Mutex<Option<(u64,String)>>` 槽，worker 永不阻塞；代际在槽锁内发布/校验，`open_updates` 失败回退的瞬态 `current` 值因互斥对 worker 的锁内 load 不可见（worker 校验与 `update_result` 都先取同一把槽锁），旧代际结果既写不进也读不出；忙时如实返回 false；浏览器仅对“最新代际且发现新版”打开。协议自洽，无新竞态。
- **m3 CLOSED**：dev 版按去后缀 core 三段比较，高于稳定版不降级（返回“已是最新”），同号 dev 视为可升级到稳定版；真正畸形版本仍报错（fail-closed）。
- **m4 CLOSED**：删除 usPostUninstall 的提前 CloseHandle/DeleteFile，锁保持到 `DeinitializeUninstall`，首轮“释放后-目录删除前”竞窗消除；遗留零字节 install.lock 与空目录为声明的取舍，重装时 CREATE_ALWAYS 可正常复用。
- **m5 CLOSED**：`GetLongPathNameW` 归一（32768 缓冲，失败退化 ExpandFileName+小写）加双形态引号解析，短路径/大小写失配不再导致漏改写或卸载残留孤儿项；对不存在路径退化后失配仍取保守“非自有”方向。
- **m7 CLOSED**：`/Sbeek=$q…$q` 为 ISCC /S 的文档化引号转义，PowerShell 侧参数不再含内嵌引号，5.1/7.x 均可原样传递；Capabilities 改异步排空 stdout+5s 上限+不重定向 stderr，死锁与挂起消除；`-or` 短路使 SignerCertificate 判空前置于取 Thumbprint；`native_machine -ne 34404` 与 Inno `x64os` 一致收紧原生 x64（缺字段即 throw，fail-closed）。

## 未关闭（重大）

- **m6 OPEN（部分）**：互斥机制本身已验证成立——应用进程以 `FILE_SHARE_READ` 共享读持有 install.lock 生命周期，安装/卸载的 share=0 独占打开与之互斥，跨会话检测与“BeeK 正在运行”文案均真实化。**但由此派生出 purge 卸载路径的确定性冲突**：beek.iss 的 `CurUninstallStepChanged(usUninstall)` 先取独占锁（m4 又将持有延至进程退出），随后 `Exec(beek.exe --purge-user-data)`；该子进程是带 installed.marker 的已安装进程，按关闭说明自述的“每个已安装进程生命周期持有 guard”不变量，其 `install_guard()`（lifecycle 实现，附件 316-334 行）对 share=0 锁的共享读打开必然失败，子进程非零退出 → `RaiseException('BeeK data cleanup failed')` → 卸载中止且其后的 Run 项清理被跳过。触发路径：卸载时对数据清除对话框选“是”，每次复现。除非 `--purge-user-data` 在参数分发期于 guard 之前处理——该调用点证据未附，而其正是核实 m6 关闭的必要依赖。**最小修复/关闭条件**：purge 分支在 `install_guard()` 之前分发并直接执行退出（或提供等效调用点证据）；不可反向让卸载器在 Exec 前放锁，否则复现 m4 竞窗。

## 新引入 Bug 自查

m1/m2/m3/m5/m7 差异未发现新的可触发问题（含 `wrapping_add`、槽锁内 try_send 无阻塞、StrictMode 下属性访问等点均已核对）。唯一新派生问题即上述 m6→purge 冲突。次要注意：`install_guard` 返回的 `File` 须由调用方全程持有，丢弃即提前放锁，属调用点信任，随 m6 一并取证。

## 边界声明

G5 未通过、不可公开发布；未签名预览、外部实测等保留阻塞不计入本轮。修复 purge/guard 分发顺序（或附证据）后，本子系统可复审转有限 PASS，无需重验其余六项。
