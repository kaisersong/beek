# R7 复审（Chromium 拖选锚点与点击复核）

**结论：源码层有限 PASS。** 锚点改用同源手势起点修复了“松开远离字形导致合法拖选被 20px 校验拒绝”的缺陷；点击复核 fallback 点改用缓存选区 rect 中心，消除了旧实现必然命不中源文字（落在蜜蜂按钮上）的问题。未发现新的可触发缺陷；回归证据未出，G5 不批准。

## 核实要点

**1. 锚点路径——正确。** `pressed = WM_LBUTTONUP && DRAG.swap(false)` 在 `external(h)` 判定之前计算，任何 UP（含落在本进程窗口）都消费 DRAG，杜绝跨手势泄漏；DOWN 侧记录 root/PID/point，UP 仅在同 root/PID 且确有未释放 DOWN 时采用起点，否则回退终点（跨窗口拖动行为与旧版一致）。键序/可见性（Release/Acquire）、多按钮不触发、`MOUSE_INPUT` 键盘复位、500ms 锚点 TTL、point 打包均保持不变。拖选场景下起点必在首个选区 rect 内，±20px 校验通过，修复成立。

**2. 点击复核——正确且失效安全。** fallback 点取 `s.rects.last()` 中心，仅作 focused 无 TextPattern 时的第二候选；命中错位（布局滚动、蜜蜂窗覆盖 rect 中心）会被 `inside_window`/pid 门拒绝，最终由 `same_selection`+fresh epoch 复核拦截，失败方向是隐藏蜜蜂而非误翻译。无 rects 时回退 `util::cursor()`（旧语义，可接受）。

**3. helper 侧门禁未松动。** 前后双 `is_current`、密码元素 Protected 短路、GetSelection/GetText/rects/runtime_id 全部有界（128/2×MAX_CHARS/1024/128）、is_url 拒绝、`pointer_anchor` 下无 rect 时合成 1×1 锚点——与既有评审结论一致，点击前零联网保留。

## 记录（不阻塞）

- **已知取舍**：起点在空白、终点落在文字上的反向拖选（浏览器中此类手势通常不产生选区）现会被 20px 校验拒绝、不弹按钮——与附件声明的设计方向一致，非缺陷。
- 非外部 UP 不清 `MOUSE_DOWN_*` 静态量，但被 `pressed`+同 root/PID 双门钳制，下一次手势 DOWN 必然覆写，无可触发路径。
- 实测回归仍在运行，按附件声明不计为完成；测试驱动限制（仅公开 fixture、窗口内坐标）为测试面约束，本方未执行。

**判定：R7 增量有限 PASS；回归证据待出后再计入环境验收；G5 未通过，不批准公开发布。**
