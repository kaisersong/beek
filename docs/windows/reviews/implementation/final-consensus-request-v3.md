# R7 v3快照绑定

请基于刚完成R7有限PASS与v2历史，简短确认Codex+ZCode复合有限源码共识，不执行工具，不重做全文。

- 产品清单SHA `6b82e5800b8d74e0cfcd953f4e2eb86f6eb22a0ca31ce5da3404e2c8c3897503`；完整源码指纹 `297097d3a99149115e52114759a04877db45efba618849ca5dde8190bb36b75c`；shipping build ID `d220c1ed48ec148e45a2eb8bc1c8f331b89bfaeef6572373eb2cc1d3381b9896`。
- Codex逐文件重算：v2后仅native.rs、ui.rs和测试desktop-agent.ps1变化，与你刚审R7一致。散列不是由你独立重算。v5方案和其余probe未变。
- 7项实机回归通过：五种有效Chrome普通段落选词（含正反向松于空白、中文）均显示按钮，延迟1100ms点击后卡片与SSE正常；点击前0新增、后恰1请求；空白和密码不触发。截图已查看。Windows23交互测试、Mac检查与Python22项通过。你的实测独立执行范围为0，仅引用Codex记录。
- 原eaa621公开同页远处松开失败保留；Edge现有profile窗口尺寸被改造成坐标失效，明确inconclusive。不能归因为产品故障，也不算通过。
- 已知起点在远处空白的手势边界如R7保留。G0–G5未全通过，未签名预览，不批准公开发布。

请引用新清单SHA与buildID给出简短最终判定。
