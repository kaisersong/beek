# R5 复审（R4 唯一 Minor 核销）

**结论：Minor CLOSED，子系统维持有限 PASS，本轮未发现新引入缺陷。** G5 环境验收仍未通过，不得公开发布。

## 核实过程

**1. 尺寸同步覆盖 keep_accessible——成立。** `keep_accessible` 现在在与 R4 弹窗路径相同的次序下工作：装子类 → SetWindowPos（含尺寸钳制）→ PostMessage(WM_APP+23)；回调在 UI 线段借用结束后读取当下 GetClientRect 并以正确 LOWORD/HIWORD 打包转交原窗口过程。R4 缺口（设置窗/快照在 `w.update()` 借用内缩尺寸导致 GPUI viewport 陈旧）由此闭合。

**2. 普通窗口激活不受损——成立。** 子类 userdata 区分两类窗口：`window_adapter` 仅在 `no_activate != 0` 时拦截 WM_MOUSEACTIVATE。`show_popup` 先写入 WS_EX_NOACTIVATE 再以 refdata=1 装子类（与 R4 行为等价，无回归）；`keep_accessible` 按窗口当前实styles计算 refdata——设置/快照窗口为 0，鼠标激活走 DefSubclassProc 原链路，正常激活语义保留。不存在“弹窗拿到 refdata=0”的路径（样式在任何子类安装前已写入，且从不清除）。

**3. 最小化防护——成立。** WM_APP+23 处理前先 `IsIconic` 跳过，避免给最小化窗口合成 SIZE_RESTORED 造成错误 viewport；恢复时系统会投递真实 WM_SIZE 自行同步。GetClientRect 失败仍安全返回 0，下次 post 自愈。

**4. 边界复查无新缺陷。** 两处均用子类 id=1，同窗口重复安装是替换而非堆叠；同一 id 下 refdata 以最后一次安装为准，且两类安装来源对同一窗口计算的 refdata 一致。系统 epoch 周期性调 keep_accessible 会重复 post 合成 WM_SIZE，尺寸未变时幂等无副作用。`move_popup` 带 SWP_NOSIZE、`activate` 的 SW_RESTORE 不产生净尺寸变化，均无同类借用风险。PostMessageW 失败经 `?` 传播后被调用方忽略，窗口仍已完成定位，仅缺一次同步，属可接受降级。

## 遗留状态

- 源码层面：R1–R5 全部问题关闭，无未关闭的源码缺陷；假设清单（`process::RestartChild/Jobs`、`util::Handle`、`util::is_current`、GPUI 合成 WM_SIZE 处理——后者已有实机佐证并本轮机制一致）维持 R4 记录不变。
- 环境层面：多显示器、浸泡、签名等仍属 G5 范畴未完成，本有限 PASS 仅覆盖附加源码，**不批准公开发布**。