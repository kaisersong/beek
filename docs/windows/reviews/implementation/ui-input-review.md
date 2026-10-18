Session: sess_de3c3049-604c-435c-b808-7a0e202fec62

# Windows UI/输入子系统 独立对抗性评审报告

**范围**：仅附件内四个文件（`src/platform/windows/ui.rs`、`input.rs`、`text_model.rs`、`native.rs`）。协调器/selection.rs、util.rs、store.rs、stream.rs、speech、lifecycle 未附，凡结论依赖这些文件的均明确标注。关联层约定（Copied 携带 clipboard-sequence、消息有界 FIFO、helper 验证原 sequence 且仅自身串行写入链可重绑、UIA 整体 1500ms）作为既定契约采信。

**结论：REVISE**（0 Blocker / 3 Major / 8 Minor）。无数据损坏或挂死级问题，但 3 个 Major 均为用户可稳定触发的错误反馈或功能缺陷，修完即可给该子系统有限 PASS。G5 未过，本轮不构成发布签核。

---

## Blocker

未发现。队列有界性全面达标：`erx`=32 且 wndproc/钩子内全部 `try_send`（钩子不阻塞）、`rx`=32、`bg_rx`=2、`work_tx`=1、`edits`≤4 溢出有提示、`edit_active`/`pending`/`mailbox`/`cache`/`bee`/`panel` 均单槽，worker 的阻塞 `send` 只反压自身线程，无死锁路径（主线程仅在 spawn worker 之前持 store 锁）。

## Major

**M1. 自身剪贴板写入回声未过滤 → 开启“复制后显示”后每次复制译文都弹多余蜜蜂**
`native.rs::procedure`（WM_CLIPBOARDUPDATE 分支，快照 2013–2020 行）只做 `CLIPBOARD_SEQUENCE` 去重，不判断更新是否来自我们自己的 ClipboardWriter 串行写入链。触发路径：设置中开启 `copy_to_popup` → 点击面板“复制译文”（面板是 NOACTIVATE，前台仍是源应用）→ helper 写入剪贴板 → owner 窗口收到 WM_CLIPBOARDUPDATE → `Event::ClipboardChanged` → 150ms 后 `util::current(epoch)` 命中源应用 → `Purpose::Clipboard(true)` 通过全部 epoch/is_current 复核 → 在选中位置弹出“来自剪贴板 · 仅本地预览”蜜蜂。设置页内 Ctrl+X/Ctrl+C 同理（是否触发取决于 `util::current` 对自有窗口的行为，util.rs 未附）。
**最小修复**（纯 ui.rs 可落地的过渡版）：在 `Event::ClipboardChanged` 分支中，若 `pending` 为 `Purpose::Copy`、或 `edit_active` 为 Copy intent、或距最近一次 `Outcome::Copied` 不足 ~300ms，则不置 `copy_requested`。根治版需写入侧配合：helper 在写入完成时把新 sequence 发布到共享原子环（即契约中重绑机制已维护的状态），`WM_CLIPBOARDUPDATE` 命中该环即跳过（写进程是独立 helper，用 `GetClipboardOwner` 查 PID 无效）。

**M2. 快速连续 Ctrl+V：第二次粘贴被静默丢弃，且状态报告“已完成”**
`ui.rs` 主循环 edit 解析臂（快照 745 行）+ `input.rs::TextInput::paste_value`（1515–1525 行）。两次 Ctrl+V 相隔小于一次往返（16ms tick + 协调器 FIFO + 1500ms UIA 预算，现实约 50–300ms）时，两个 Paste intent 携带相同 `ticket=revision`；第一次应用后 `replace_text_in_range` 将 revision +1，第二次 `paste_value` 的 `self.revision == ticket` 失败被丢弃，但 `success` 只看 `(Paste, Selected)` 结果类型，状态两次都显示“剪贴板操作已完成”。代码对 cut→paste 链已有重绑循环（746 行），恰恰漏掉 paste→paste 链。
**最小修复**：让 `paste_value` 返回 `Option<u64>`（应用成功时返回新 revision），在 `(Paste, Selected)` 解析臂中仿照 cut 的重绑循环，把该字段队列中 `ticket==旧值` 的后续 Paste 重绑到新 revision。

**M3. 面板“复制译文”失败完全无反馈**
`ui.rs` 结果处理（快照 764–766 行）：`Purpose::Copy` 只有 `Outcome::Copied` 会更新面板状态；helper 因 sequence 校验失败（按钮按下到 helper 校验之间剪贴板被剪贴板管理器等改写）或剪贴板被占用而返回 `Outcome::Empty/Fault` 时，仅清除 `pending` 并 hide bee，面板停留在旧状态（如“完成”），用户以为已复制。另 `Command::Copy` 处理器（782 行）在 `util::source` 返回 None 时静默丢弃命令，同样无反馈。
**最小修复**：`Empty`/`Fault` 臂改为先 `pending.take()` 再按 purpose 分支——若为 `Purpose::Copy`，向面板写“复制未完成，剪贴板内容已变化，请重试”；`Command::Copy` 的 source-None 分支给同样状态。

## Minor

- **m1. Verify 不匹配时蜜蜂卡在 loading**（`ui.rs::Purpose::Verify`，759 行）：复核内容与点击时不一致时什么都不做，`pending` 已消费，蜜蜂停在禁用态吞掉点击，直到 cache TTL/epoch 变化才隐藏。修复：else 分支 `hide(&mut bee, cx)`。
- **m2. edit 派发“先 pop 后判 source”，失败即静默丢弃**（`ui.rs` 788–791 行）：设置窗口瞬时非前台（或刚被关闭，`form` 残留过期句柄）时，被 pop 的 Ctrl+X/Ctrl+V 无派发、无提示，且每 tick 丢一个。修复：改用 `edits.front()` peek，派发成功才 pop。
- **m3. 成功状态谎报**（`input.rs::paste_value/cut_complete`，1501–1525 行）：revision 不匹配/失焦/选区变化时二者内部 no-op，但状态仍写“剪贴板操作已完成”。cut 场景更明显：剪贴板已写入（Copied）而字段未删除，字段与剪贴板同时保有文本。修复：让二者返回是否实际应用，未应用时状态写“内容已变化，操作已取消”（cut 的部分成功单独表述）。
- **m4. SYSTEM_EPOCH 复位丢弃在途 edit/pending 无提示**（`ui.rs` 713 行）：锁屏/切显示器瞬间按下的 Ctrl+V/Ctrl+C 静默消失。修复：复位分支的 `cx.update` 里向 form 写一条“系统状态变化，剪贴板操作已取消”。
- **m5. 保存期间关闭设置窗口则保存结果（含失败）丢失**（`ui.rs` `Background::Saved` 793 行 / `Event::Settings` 720 行）：窗口内反馈链路（busy、错误、autostart 警告、HOTKEY_OK 提示）是完整的；但关闭后重开传入空 status，磁盘失败无人知晓。修复：循环内保存 `last_save_result: Option<String>`，重开 form 时作为初始 status。
- **m6. `range_from_utf16` 不钳制倒置区间**（`input.rs` 1085–1087 行）：`text_model::utf16_range` 有 `.max(start)` 钳制，此函数没有；异常 IME 给出 end<start 的 UTF16 区间时 `replace_text_in_range` 的 `range.end - range.start` 下溢/切片 panic。修复：与 `utf16_range` 同样钳制。
- **m7. 延迟粘贴落进未提交的组合文本**（`input.rs::replace_text_in_range` 1165 行 `marked_range.take()`）：粘贴被推迟最多 1500ms，落在 CJK IME 组合期间的概率远高于上游同步示例；清 marked_range 不通知 TSF，后续提交可能重复拼入组合文本。修复：`paste_value` 在 `marked_range.is_some()` 时直接跳过并按未应用上报（与 m3 的返回值改造合并）。
- **m8. 窗口创建 `expect` 直接 panic**（`ui.rs` 493/527/593/701 行）：GPU 驱动复位等场景 `open_window` 返回 Err 即崩进程；另 720 行 `&env!("BEEK_BUILD_ID")[..12]` 在构建 ID 短于 12 字符时 panic（依赖未附的 build.rs，防御性问题）。修复：降级为日志 + 跳过该次弹窗；切片改 `get(..12).unwrap_or(...)`。

另记一处非缺陷：`Event::Clipboard` 臂内 `let auto = matches!(event, Event::ClipboardChanged)` 恒为 false（ClipboardChanged 已被前一个臂消费），属死代码，自动路径实际经 `copy_requested` 正常工作。

## 各关注点判定

- **异步 Copy/Cut/Paste 与快速连续操作**：REVISE（M2、M3、m2、m3）。cut→paste 重绑、失败剪贴板 purge（`failed_copy` + `at<=failed` 门）、edits≤4 限流设计本身是对的，缺口集中在 paste→paste 链与失败反馈。
- **UI 线程/事件队列有界**：PASS（见 Blocker 段落）。
- **焦点与 IME/UTF16**：有限 PASS。text_model 的代理对钳制有测试覆盖、`offset_to_utf16` 正确、masked 显示按字节等长重复是有意保安全的做法；遗留 m6、m7。
- **选区 epoch/点击复核/旧网络响应隔离**：PASS。双 epoch（EPOCH/SYSTEM_EPOCH）、点击复核（cache `same_selection` + epoch + `is_current` + 协调器复读）、generation+`cancel_pending` 覆盖所有失效路径、`Background::Request` 的 generation+epoch+panel 存活三重门、快照面板的 epoch 冻结是有意设计且有 panel 校验兜底，未发现旧响应可写入新界面的通路。
- **Settings 保存失败反馈**：有限 PASS（窗口内链路完整，m5 补关闭窗口的缺口）。
- **live 无激活与普通键盘预览**：PASS。live 弹窗 `focus:false` + `WS_EX_NOACTIVATE` + `MA_NOACTIVATE` 子类化，点击/拖拽均不夺焦，键盘留在源应用；热键路径走激活的快照面板，Enter/Esc/Ctrl-C 绑定与 `!active` 防重入正确。

**放行条件**：修复 M1–M3（m1–m3 建议同批）后本子系统可给有限 PASS；对外发布仍以 G5 全量通过为前提，本轮不签核。
