# Windows 方案评审材料

入口：[详细方案](../windows-port-plan.md) · [最终共识](consensus.md) · [问题台账](issue-ledger.md)。

用户已明确选择Codex + ZCode两方一致收敛。v5已获Codex与ZCode对同一SHA的明确PASS，两方设计一致已达成；xiaok和Kimi不计入最终席位。本目录保存设计审查证据，不能证明Windows版已实现或实测。

| 材料 | 用途 |
| --- | --- |
| [最终共识记录](consensus.md) | 双方同SHA签核、范围和未运行门禁 |
| [v5冻结快照](round-5-plan.md) / [SHA](round-5.sha256) | 最终签核快照，与主方案逐字一致，保留候选稿原始版本文字以免破坏散列 |
| [Codex v5](round-5-codex.md) | 同SHA独立PASS，仅设计 |
| [ZCode v5结论](round-5-zcode.md) / [运行记录](round-5-zcode-run.json) | 第三轮PASS，206.4秒正常完成 |
| [ZCode第三轮输入](round-5-zcode-prompt.txt) / [原始日志](round-5-zcode-raw.log) | 真实本地CLI复审；GLM-5.3 / max |
| [ZCode第二轮](round-4-zcode.md) / [修订答复](response-to-zcode-r2.md) | v4新增2项Major和1项Minor及v5修订 |
| [ZCode首轮](round-2-zcode.md) / [修订与反驳](response-to-zcode-r1.md) | 8项问题，接受问题但反驳遗留卡死MTA线程方案 |
| [Codex首轮](round-1-codex.md) / [v2追加反例](round-2-codex-addendum.md) | 6项初始问题、2项自审追加；追加意见覆盖v2初始PASS |
| [Codex v4](round-4-codex.md) | 当时单方PASS；后来ZCode发现新反例，不能冒称v4共识 |
| [实际ZCode身份](round-2-zcode-identity.json) | 本任务会话、模型、CLI版本及推理配置 |
| [xiaok诊断](xiaok-call-diagnosis.md) | 默认模型、实施意图误判、持续thinking与约300秒断流的实际证据 |
| [Kimi原始日志](round-1-kimi-raw.log) / [运行记录](round-1-kimi-run.json) | 实际配额403，没有评审意见 |

历史round-1/2/3/4-plan.md和.sha256保留原文及散列。v3为未提交外审的预冻结快照；ZCode实际完成的轮次以v2、v4及本轮v5为准。raw.log为原始CLI输出，清理版.md仅提取最终评审正文（插件状态和误选工具的过程保留在raw.log）；prompt.txt为实际输入，run.json为退出及耗时记录。旧提示中关于评审阵容的文本是历史输入，最终阵容以用户最新选择及主方案§12为准。

`round-1-xiaok-attempt-*`、`round-1-xiaok-native-*`、`round-part5-xiaok-native-*`、`round-2-xiaok-stream-*`、`round-4-xiaok-stream-*`及progress JSONL均为诊断，不能代替意见。`round-2-kimi-prompt.txt`仅准备过，未执行，不计为第二次有效评审。smoke只证明短请求连通。目录中的runner脚本没有嵌入凭据；不使用它们修改全局配置。

协作状态使用[Intent Broker skill](/Users/kk/.codex/skills/intent-broker/SKILL.md)检查；实际模型判断以原始响应为准。
