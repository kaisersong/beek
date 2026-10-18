# 2026-10-18 Chrome取词与点击复核

用户反馈：Notepad可以，Chrome不弹按钮。已修复公开普通网页上实际复现的拖选缺陷，并安装内部006到ilucas。用户原始网页地址尚未提供，不能将公开夹具结果等同于原页面已通过。

## 原因与变更

旧005（build `eaa6216491a0f4efaa32daea37c6aa7e0e525abd99aeefa677fc0e523019053c`）在Chrome154公开普通段落上，双击、贴近文字结束拖动可以弹出并点击卡片；从文字内起拖、松到右侧空白处时，网页确有蓝色文字选区，但按钮不出现。原始截图与结构化记录在本地 `test-results/ilucas/chrome-whitespace-original.json`。UIA查询用鼠标松开点作锚，helper要求锚点位于选区矩形20px内，合法拖动因此被拒绝。

- `native.rs`记录鼠标按下点的root/PID；松开时，仅同一源窗口/进程且确有未释放按下，才使用手势起点验证。其他情况保留终点。任意鼠标抬起均消费DRAG，防止跨手势遗留。
- `ui.rs`点击后的新鲜UIA查询，fallback点改为缓存选区矩形中心。原来当前鼠标已经位于蜜蜂按钮上，focused元素没有TextPattern时fallback可能无法命中源文字。仍须新鲜来源、epoch及same_selection一致才联网，没有绕过复核。
- 测试驱动补充实际double/drag动作，要求公开夹具前台和窗口内坐标。产品未增加模拟复制或浏览器扩展。

## 本轮实际结果

使用真实Windows release配对，Chrome154.0.8037.98，新建独立测试profile，没有强制accessibility启动参数。普通段落使用真实鼠标输入，不只读取textarea。

| 场景 | 结果 |
| --- | --- |
| 英文双击词、正常拖选 | 按钮、点击后卡片及SSE通过 |
| 正向拖选松在右侧空白、反向拖选松在左侧空白 | 旧问题场景在新版通过 |
| 中文拖选松在空白 | 按钮、点击后卡片及SSE通过 |
| 五个有效选区均延迟1100ms再点击 | 超过500ms鼠标锚点时效后仍通过；源浏览器保持前台 |
| 上述每次点击前/后请求差值 | 点击前0，点击后恰1，译文截图已查看 |
| 空白点击、密码字段选择 | 均无按钮、无新增请求 |
| Notepad原流程 | 真实选词、延迟点击、SSE卡片通过，源窗口保持前台 |
| Windows构建/测试 | release构建通过；session0 all-target 22通过/1忽略；交互session1含真实Vault测试23/23、原生exit0 |
| 宿主检查/工具 | macOS all-target check（dev-runtime-shaders）、Python工具22项通过；macOS核心19/19通过，日志单独保留 |
| 安装 | 006普通用户安装exit0，新不可变版本程序已启动 |

本地证据：`test-results/ilucas/chromium-gesture-r7.json`、`notepad-after-chromium-fix.json`、`product-tests-interactive-chromium.json`、`chromium-install-restore.json`；宿主日志在 `test-results/host/*chromium.log`。这是本轮7项针对性Chrome回归及Notepad补充，不是完整87项验收。

Edge现有profile首轮中窗口由最大化恢复，固定坐标随之失效，又出现额外页签。该记录 `edge-user-profile-gesture-original.json` 明确标为inconclusive，不能归因产品或记通过；没有关闭含额外页签的浏览器窗口。

## 凭据与配置恢复

测试前正常退出BeeK，备份原配置字节，测试配置去掉凭据引用且限定loopback。测试进程仅通过自身环境读取公开假Key，真实Vault条目没有替换或删除；未使用真实Key发请求。

测试后正常退出测试进程，在配置锁及并发变化检查下原子恢复原配置，SHA256与备份完全一致；默认模型、官方API地址、原凭据引用、自启动设置均保留。交互session1确认原Vault条目仍存在。临时配置备份在恢复验证成功后移除；新版设置仍显示已保存星号。未将测试模型、地址或假Key留在用户配置中。

## 冻结、评审及交付

- 新build ID：`d220c1ed48ec148e45a2eb8bc1c8f331b89bfaeef6572373eb2cc1d3381b9896`。
- [v3清单](reviews/implementation/final-source-manifest-v3.json) SHA256：`6b82e5800b8d74e0cfcd953f4e2eb86f6eb22a0ca31ce5da3404e2c8c3897503`；完整源码指纹 `297097d3a99149115e52114759a04877db45efba618849ca5dde8190bb36b75c`。
- [Codex自审](reviews/implementation/final-codex-v3.md)、[ZCode R7实质复审](reviews/implementation/chromium-review-7.md)及[v3绑定判定](reviews/implementation/final-zcode-v3.md)：有限源码共识PASS。ZCode没有独立执行测试或重算散列。
- 本地交付目录 `dist/windows-preview-20261018-chromium/`，未签名内部预览006；ZIP SHA256 `8381b8aa60cab03061b11a1e2a7b6d98c0ebd30b439f467225425b3dd3a9a913`，setup SHA256 `55ae91771110f7001e9e35b4350a5950877e3f12f7c5d04eeaa8b5f83031110f`。本地文件及ZIP逐文件manifest散列已复核。

边界：从远处空白开始的拖动仍受起点距离门禁约束。未验证用户尚未提供的原网页、所有网页/PDF/远程桌面。完整G0–G5、多屏/拔插、签名、干净机、第二GPU、8小时等缺项仍阻止正式发布；未提交、推送或发布。
