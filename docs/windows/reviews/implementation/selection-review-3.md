复核完成（附件 330 行已完整读取）。**结论：维持有限 PASS**——selection 子系统（UIA/进程/协议/退避）无未关闭重大问题；R2.n1 经真实调用链核实为有意设计，撤回 Minor 定级。

**剪贴板写入尾部（实测修复）**：正确。CloseClipboard 后才读发布序号并回填 `own_after`，与 helper 读回定序逻辑自洽；返回前双读序号+属主校验，竞态写入判 SourceChanged，有界且保守。哨兵先行、弱断言标记 inconclusive 的测试处理合理。

**R2.n1（INFINITE 等待）**：我 R2 的担忧源于当时依赖未附（假设其 Drop 落在 UI/退出路径）。实际链路核实：`RestartChild` 仅存在于 `restart()` 派生的后台监督/创建线程局部，未存入 `self`；`Instance::drop` 不 Drop 该对象、不 join 这些线程，仅 cancel+kill 候选作业，主退出等待有界（1100ms 监听线程+互斥释放）。内核级卡死的候选会永久占用唯一的 Busy 槽，禁止无界重试候选——错误文案已明示此意图。创建线程 5 秒超时兜底。判定：**有界的资源设计，n1 按 CLOSED（设计性）处理，非缺陷**。

**R2.n2**：已由 `Jobs::limited(1, None)` 单次设置解决，内存上限仅对 helper 作业生效，无双重覆写。CLOSED。

**新附依赖（完整性级别）**：`token()` 新增 TOKEN_MANDATORY_LABEL 解析，尺寸上限、usize 对齐、`count==0` 守卫、末位 sub-authority 取值均正确；`eligible` 增加 `other.integrity <= own.integrity` 与 UIPI 方向一致（非提升自身为 Medium，Low/AppContainer 目标不受影响），无误杀常规应用，缓存键与容量不变。未发现可触发缺陷。

**边界说明（非本子系统缺陷）**：`restart()` 的 `restart_events` 置 Some 后附件内未见失败路径的清理点，后续 `restart()` 是否被首条件永久拒绝取决于未附的事件消费方，请在 lifecycle 侧评审确认；剪贴板实测结论采信附件自述，非本次独立复测。

**范围与限制**：本 PASS 覆盖三轮快照内 selection 子系统源码正确性；G5 仍未通过，签名/多屏/干净机/8 小时证据仍缺，不构成可公开发布判断。