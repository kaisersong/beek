# BeeK Windows 方案评审问题台账

## 当前状态

用户明确选择“按 Codex + ZCode 两方一致收敛”。本轮只按这两个真实参与者签核；xiaok、Kimi没有有效意见，不计为通过，也不再等待其恢复。

- 最终候选：v5，SHA-256 `204073460c96a1c08c786759a4ea571a795e00d16fa9d5ac3e45916e02151951`。
- Codex：v5 **PASS**；ZCode：v5 **PASS**。**两方设计一致已达成**，详见[最终共识记录](consensus.md)。
- ZCode实际使用本地CLI 0.16.9、GLM-5.3 / max；会话身份见[实际记录](round-2-zcode-identity.json)。
- 本轮提出19项设计异议：12项Major、7项Minor、0项Blocking。问题关闭表示设计契约和未来证据安排已修订，不代表测试已经通过。
- Windows业务实现、构建、真实桌面测试、性能、签名及发布均未执行；没有人天估算。

## 全部异议

| ID | 严重性 | 问题 | 最终修订位置 | 设计状态 |
| --- | --- | --- | --- | --- |
| CODEX-R1-01 | Major | 翻译工作/取消/流队列无界 | §6.3 两个真实槽、一个pending、输出与SSE上限、逻辑取消 | 关闭（v5双方PASS） |
| CODEX-R1-02 | Major | 重启与单实例交接竞争 | §8 父子身份、预就绪握手、锁交接和失败保留旧实例 | 关闭（v5双方PASS） |
| CODEX-R1-03 | Major | 剪贴板前台不等于来源 | §4/§6.1 独立来源、关联失败跳过、显式预览 | 关闭（v5双方PASS） |
| CODEX-R1-04 | Major | 所有应用降至L2仍可宣称完成 | §10.3 记事本/Edge/Chrome最小L1硬门槛 | 关闭（v5双方PASS） |
| CODEX-R1-05 | Minor | 安装器选型留白 | §9.1 预选Inno Setup、用户级安装及升级/卸载协议 | 关闭（v5双方PASS） |
| CODEX-R1-06 | Major | 同HWND切页后旧文本可继续翻译 | §5.1 epoch/TTL/选区身份、点击前复核 | 关闭（v5双方PASS） |
| CODEX-R2-01 | Major | 配置原子写不能替代跨凭据存储事务 | §8 不可变新引用暂存、读回、整配置提交、回收 | 关闭（v5双方PASS） |
| CODEX-R2-02 | Major | G0原型隐含依赖G1完成 | §3 独立probe crate与锁文件，完整BeeK构建留G1 | 关闭（v5双方PASS） |
| ZCODE-R1-01 | Major | 慢UIA来源/剪贴板可击穿全局 | §5.3/§6.1 可回收UIA工作进程、来源隔离、独立reader | 关闭（v5双方PASS） |
| ZCODE-R1-02 | Major | G0未覆盖可激活输入和中文IME | §3/§11 前置IME、键盘和输入门禁 | 关闭（v5双方PASS） |
| ZCODE-R1-03 | Major | 普通全屏与放映行为未定义 | §7/§10.3 F11选区、失效/重定位及放映边界 | 关闭（v5双方PASS） |
| ZCODE-R1-04 | Minor | Windows-only stable影响旧macOS更新检查 | §9.1 双平台完整资产、聚合发布门禁 | 关闭（v5双方PASS） |
| ZCODE-R1-05 | Minor | IPC缺传输/命名/ACL/白名单 | §8 有界本地命名管道、用户会话、nonce及身份校验 | 关闭（v5双方PASS） |
| ZCODE-R1-06 | Minor | G2未有产品UI却要求产品证据 | §11 固定公开fixture只读后端harness，G3/G5正式UI | 关闭（v5双方PASS） |
| ZCODE-R1-07 | Minor | Windows无launchd级watchdog却称常驻 | §8 明确无崩溃自动恢复，Job清理和手动恢复 | 关闭（v5双方PASS） |
| ZCODE-R1-08 | Minor | 取消语义与pending替换/退出不完整 | §6.3 明确逻辑取消、等待显示、替换和2秒退出上限 | 关闭（v5双方PASS） |
| ZCODE-R2-01 | Major | 单源worker崩溃不进退避且持续重建 | §5.3 绑定源/回调崩溃计数、第四次隔离、总创建率 | 关闭（v5双方PASS） |
| ZCODE-R2-02 | Major | 双UIA槽/reader满载时新请求无契约 | §5.3/§6.1 失效抢占、最新pending、300ms等待及Busy | 关闭（v5双方PASS） |
| ZCODE-R2-03 | Minor | 第二次启动行为与IPC白名单不一致 | §8 统一ActivateSettings；§10.2对应断言 | 关闭（v5双方PASS） |

## 真实审查轮次与覆盖关系

| 快照 | Codex | ZCode | 当时结论 |
| --- | --- | --- | --- |
| v1 | REVISE，6项 | 未审 | 修订 |
| v2 | 初为PASS；追加2项后覆盖为REVISE | 首轮REVISE，8项 | 修订 |
| v3 | 预冻结修订快照 | 未送审 | 不能计作已完成外部轮次 |
| v4 | PASS | 第二轮REVISE：接受此前10项修订，新增3项 | 继续修订，未达共识 |
| v5 | PASS | 第三轮PASS | 同SHA两方设计一致 |

Codex v1六项在后续版本保留并经双方全文复核；ZCode v4明确关闭其首轮8项和Codex追加2项。v4的Codex PASS不能覆盖ZCode的新反例；v5修订映射见[第二轮答复](response-to-zcode-r2.md)。关于拒绝“保留卡死MTA线程”的技术反驳及ZCode接受替代方案的实际意见，见[首轮答复](response-to-zcode-r1.md)和[ZCode第二轮全文](round-4-zcode.md)。

## 快照散列

- v1：`4d22415b8955bb0897b95f99bd994ba3cf25ff10760beb150b62df12ad242a4d`。
- v2：`3677663c2fa62ea9770d6bf0f3aa15ac730387a6335bb9f5a805ad1314675048`。
- v3：`9ad552bd1f234c36fc99633bb3c138407f049a1b49ca4269e756921022e5fa64`。
- v4：`673372b47419af95794c275ba068eb6d003cc919926910cd7f34c2ee99ea047e`。
- v5：`204073460c96a1c08c786759a4ea571a795e00d16fa9d5ac3e45916e02151951`。

## 缺席者与过程异常

- xiaok实际默认模型`qwen3.8-flash-next`。前两次CLI误入实施编排；后续固定时限调用未取得正文，不能断言服务无响应。进度感知调用持续约300秒生成thinking后以`ERR_STREAM_PREMATURE_CLOSE`结束，0正文，非外层超时；按次关闭thinking未被当前链路兑现。详见[诊断](xiaok-call-diagnosis.md)，不保存思考正文、不打印凭据、未改全局模型配置。
- Kimi CLI 2.1.1：首次参数组合被拒后改为受支持调用，实际服务端七日配额403；没有有效评审。默认`kimi-code/kimi-for-coding`，仅配置同一managed provider；未购买额度或改账户。
- ZCode每次均为本地真实CLI只读评审，使用同一已核验会话；没有由Codex扮演ZCode。只有实际响应的显式Verdict算意见，CLI成功、连通smoke或无输出不能代替PASS。
- xiaok产生的仓库能力缓存已移至仓库外临时目录，见[运行产物记录](local-runtime-artifacts.json)。未更改业务源码、用户配置、已安装CLI或提交代码。

Windows实际开发必须依次完成主方案G0–G5，G6/G7独立可选门禁；本台账不提供运行验收结论。
