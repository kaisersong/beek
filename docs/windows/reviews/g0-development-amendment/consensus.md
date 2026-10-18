# G0-D 开发准入记录

2026-10-17：Codex 与实际 ZCode（GLM-5.3/max，原会话 sess_c4d1d431-4178-447d-a512-24580670b423）对补充快照 SHA256 `fb35e43de135a5390999902d114056501729b338bbf2a1f60b9bad46b18a9343` 均给出设计 PASS。详见 codex-v1.md 与 zcode-v1-raw.log。

同日在 ilucas 控制台 session1 非提升交互桌面完成15项 release 原型验证：构建/GPU、显示及连续10次点击不抢焦点且记事本原选区保持、明暗背景透明和全屏置顶、Unicode/微软拼音/替换/粘贴/Tab、零可见窗口36秒驻留后重开输入、退出后重启再退出、公开输入哨兵不进入日志。

被测 release exe SHA256：`9c39544abc1af70ae90d1be4c9b81209e32bf1a04e5137868ff1f3f958ed6d53`。独立证据记录 test-results/ilucas/g0-development-gate.json（stage G0-D，未改普通 validator），绑定四份 g0-release-*.json、desktop 请求/响应、截图及两份原始轨迹。轨迹检查实际通过，视觉透明与候选窗口由截图复核。全屏截图包含 Edge 首次启动提示，不据此声称网页编辑选区通过；全屏尺寸及前台/焦点保持由结构化记录证明。

发现并修复 GPUI 关闭最后窗口退出问题：采用公开 API 创建永久隐藏 lifecycle host，分别记录 visible_windows=0/gpui_windows=1，真实驻留重开复测通过。没有修改 GPUI。

**G0-D 生效，可进入 G1–G4 内部实现。完整 G0 仍未通过：G0-12/13/16 未执行。G5、正式发布和 G6/G7 未被本记录满足。** 修改 GPUI/原型前提须重新验收。
