# Codex R7 自审与v3冻结

有限源码PASS；G0–G5未全通过，不批准发布。

v2后仅native.rs（鼠标同源起点锚定、任意UP清DRAG）、ui.rs（点击复核fallback命中源选区）、测试desktop-agent.ps1（公开夹具真实double/drag）变化。鼠标hook仍只处理值，不做UIA/网络/同步IPC；同root/PID、原epoch/来源验证、密码保护和fresh same_selection门禁保留。未用缓存直接联网。

已验证旧eaa621在公开Chrome普通段落拖选并松于远处空白时有选中文字而无按钮；新release d220同动作通过。新7项实机回归覆盖英文双击、正常拖选、正反向松于空白、中文、空白点击及密码排除，5个有效选区均延迟1100ms后点击：点击前零新增请求、点击后恰一条loopback SSE，卡片译文/源窗口前台已核查。Windows交互23/23、session0 all-target22通过1忽略、Mac all-target check和Python22项通过。未使用真实用户Key。

源码清单SHA `6b82e5800b8d74e0cfcd953f4e2eb86f6eb22a0ca31ce5da3404e2c8c3897503`；完整指纹 `297097d3a99149115e52114759a04877db45efba618849ca5dde8190bb36b75c`；build ID `d220c1ed48ec148e45a2eb8bc1c8f331b89bfaeef6572373eb2cc1d3381b9896`。ZCode R7实质有限PASS，散列由Codex重算。新清单只绑定已审增量与v2历史；其余完整验收仍缺，不将本轮7项替代87项。

边界：此修复确认覆盖从文字内开始、松于空白的拖选；从远处空白开始的手势仍受起点距离门禁约束。不声称所有网页/PDF/远程桌面或用户尚未提供的具体网页都已通过。Edge现有profile首轮坐标因窗口恢复而失效，标为inconclusive，不算产品失败或通过。
