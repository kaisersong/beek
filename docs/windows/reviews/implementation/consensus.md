# Windows 实现：Codex + ZCode 有限源码共识

2026-10-17，两方对最终实现范围达成有限 PASS，所有已提出的源码问题关闭。**这不是完整 G0–G5 验收通过，也不是正式发布批准。**

## 当前冻结 v3（Chrome手势修复）

2026-10-18，v2后仅 `native.rs`、`ui.rs` 和测试驱动变化：同源鼠标手势起点作锚，任意UP清DRAG；点击后的UIA fallback命中源选区，仍须fresh same_selection才联网。复现旧版“拖选松于空白处不弹按钮”，新版7项Chrome针对性实机回归和Notepad回归通过。用户原页面尚未提供，不声称已验证。

- [v3清单](final-source-manifest-v3.json) SHA256：`6b82e5800b8d74e0cfcd953f4e2eb86f6eb22a0ca31ce5da3404e2c8c3897503`。
- 源码指纹：`297097d3a99149115e52114759a04877db45efba618849ca5dde8190bb36b75c`；build ID：`d220c1ed48ec148e45a2eb8bc1c8f331b89bfaeef6572373eb2cc1d3381b9896`。
- [Codex v3](final-codex-v3.md)、[ZCode R7](chromium-review-7.md)及[同标识判定](final-zcode-v3.md)：复合有限源码共识PASS。ZCode沿用UI会话/GLM-5.3，只审附加源码及声明一致性，未重算或独立执行测试。
- 实测与限制见[10月18日报告](../../test-report-20261018-chromium.md)。测试使用loopback和公开假Key，用户原配置已逐字节恢复、Vault存在性确认。完整87项未全部通过，不批准正式发布。

v1/v2冻结文件保持历史；方案与probe未改。起点在远处空白的手势边界按R7原文保留。

## 历史冻结 v2（保存Key显示修复）

用户指出ilucas残留测试模型/地址与已存Key外观不明确。模型/地址已一次性恢复，原凭据保留；产品只改 `input.rs`、`ui.rs`，固定八星与真实编辑缓冲分离。未编辑Key保存仍为Keep；异步保存不清除新编辑。

- [v2产品清单](final-source-manifest-v2.json)，SHA256：`e2b33455d39de93486afaf4897b25d2aef4934f1985880333d30a0e48ace7165`。
- 源码指纹：`810987747069fb8d29d9effc8ced5c8b8bfddca946defa83c5e8eaee2c28013e`。
- Shipping build ID：`eaa6216491a0f4efaa32daea37c6aa7e0e525abd99aeefa677fc0e523019053c`。
- [Codex v2自审](final-codex-v2.md)、[ZCode R6实质复审](saved-key-review-6.md)、[ZCode v2同标识最终判定](final-zcode-v2.md)：两方有限源码共识PASS，无强制修改遗留。
- 外审沿用下表UI会话，实际ZCode CLI / GLM-5.3；散列由Codex重算，ZCode引用声明值并核对一致性，没有独立重算或执行Windows测试。
- Windows交互23/23、设置星号/直接保存/重开及原凭据保留实机通过。旧003暖机和主流程复测不冒充005全量验收。完整87项仍未全部通过，不批准正式发布。

v1文件保持历史不可改写；G0 probe未变，继续引用下述补充清单。R6两条非阻塞建议保留原文，不为显示修复扩展产品范围。

## 历史冻结 v1

- [最终产品清单](final-source-manifest-v1.json)，SHA256：`584eb1f15f6b22408efd4212cec9cf2bb327b958ba0f925aee1bb83d25faadb4`。
- 完整验收源码指纹：`6110fb9473824296a8173648e9fc7eb743a5b70900beb6a2548483a3c4b30f68`。
- Shipping release build ID：`69e09ecbc3a9ca689a5b613564e67c7e8c6cd25205936a6b318ceab864d71d49`。
- 原 v5 设计 SHA：`204073460c96a1c08c786759a4ea571a795e00d16fa9d5ac3e45916e02151951`，未改写。
- 产品清单列出产品源码/测试/资源/打包文件；G0 probe 另附[逐文件补充清单](final-probe-inventory-v1.json)，也包含在完整源码指纹内。原型开发门禁有独立签核和 release 证据。

[Codex 自审](final-codex-v1.md)在最终 ZCode 判定前保存；[ZCode 最终判定](final-zcode-v1.md)明确引用上述标识并给出有限共识 PASS。散列由 Codex 本地重算，ZCode没有独立重算，不声称逐行独立审阅未附全仓。原始输出与运行元数据分别为 `final-zcode-v1.log`、`final-zcode-v1-run.json`。

## 实际审查阵容及过程

外审为本地 ZCode CLI 0.16.9，实际 GLM-5.3 / max，使用独立子系统会话和不可改写附件。未将 Codex 子代理冒充 ZCode，未把 xiaok/Kimi 的失败算作通过。

| 范围 | 实际会话 | 最终证据 |
| --- | --- | --- |
| UIA、进程、协议、退避 | `sess_70ea62df-888f-4da5-8814-eb8586320616` | [R3](selection-review-3.md) |
| 配置、凭据、生命周期 | `sess_72889fb4-eecd-4f72-877a-333c3b8d7e03` | [R2](storage-lifecycle-review-2.md)，后续调用点由R4复核 |
| 网络、打包、更新、许可清单 | `sess_4a763d88-ca54-4e3a-a975-54944c197eea` | [R3](network-packaging-review-3.md) |
| 输入、窗口、剪贴板交互 | `sess_de3c3049-604c-435c-b808-7a0e202fec62` | [R3](ui-input-review-3.md)、[R4](ui-lifecycle-review-4.md)、[R5](native-review-5.md)及最终共识 |

最早的全仓交互式审查取消后没有结论，保留 `round-1-run.json` 为 canceled_no_verdict；随后改成有边界的源码附件。初轮 REVISE 没被后续 PASS 覆盖删除。

## 异议关闭摘要

| 异议 | 关闭方式 |
| --- | --- |
| 管理性取消消耗恢复探针、静默 kill、超长选区误报、stderr污染协议 | Health::abandon、所有取消路径输出Fault、TooLong且保留健康worker、独立NUL stderr；取消恢复/挂起实测 |
| legacy Keep丢key、配置字段无界、提交等待预算不一致、备份/清理错误 | 核心事务迁移、字段限额、父子协议预算、限量备份及错误反馈；真实Vault/原子替换失败测试 |
| 自身复制回流、连按粘贴丢编辑、失败复制仍像成功、UTF-16/IME竞态 | 父侧helper PID/sequence过滤、FIFO与revision重绑、可见状态、composition守卫；DISTINCT哨兵和真实剪贴板持锁实测 |
| SSE行边界、更新线程增长、开发版本、Run短路径、卸载锁及签名命令引用 | CRLF边界测试、固定worker/队列、精确资产、路径规范化、锁持至退出、参数引用与caps超时 |
| 实时卡底部按钮不见，普通窗口可能同类问题 | 延迟到原生消息循环重新读取客户区尺寸并转交WM_SIZE；普通窗口保持正常激活、最小化不合成restore；R5关闭最后Minor |
| 语音启动阻塞且失败无反馈 | 首次使用才初始化专属STA语音线程，有界队列/回执、错误可见；实机API完成反馈 |
| 后台候选无限等待会阻塞UI、purge与安装锁冲突、helper崩溃后PID永久陈旧 | 三项均曾因缺失依赖产生假设；补充实际调用链后撤回。保留唯一真实候选Busy槽，purge早于ui::run，PID由父侧Drop清理 |

## 验收边界

实际测试结果独立见[实施状态](../../implementation-status.md)、[首批报告](../../test-report-20261017.md)及[追加报告](../../test-report-20261017-recheck.md)。003在机器低内存期间出现helper超时，保留失败；资源恢复后同包主流程和暖机空闲通过，冷启动循环被前台干扰阻塞。005设置修复后的测试单独记录。完整矩阵、签名、多屏/拔插、Explorer真实重启、干净机、第二GPU、8小时及macOS完整运行回归仍缺。运行失败可以推翻交付验收，不能用本源码共识将其豁免。v1的87项历史清点保存在本地 `test-results/ilucas/acceptance-inventory-final.json`，不将其冒充v2完整结果，也不借用旧快照批量填PASS。
