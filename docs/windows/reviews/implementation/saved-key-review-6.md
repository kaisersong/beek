# R6 复审（保存过的 Key 显示状态修复）

**结论：有限 PASS。** 星号显示与真实编辑缓冲、KeyEdit 语义已彻底分离，未发现凭据被覆盖或新编辑丢失的可触发路径；提示的三个风险面（星号误当 Key、异步保存丢编辑、索引/IME/光标）逐项核实安全。G5 仍未通过，不构成发布批准；本增量晚于 v1 冻结清单，维持附件所述“需新快照”处理。

## 分离语义核实

**星号不进缓冲。** `"********"` 仅在 `prepaint` 的显示分支出现，条件是 `masked && saved_secret && content.is_empty()`，`content` 恒为空。`value()` 只读 `content`，`submit` 的 `key.trim().is_empty() → KeyEdit::Keep` 与 R5 语义完全一致——标记串无任何通路进入 `KeyEdit::Replace`。masked 字段的 Copy/Cut 拦截未变，星号也无法被复制出去。

**状态机闭环。** 打字/粘贴后缓冲非空，显示切换为按字节星号，标记自动消失；提交 Replace 成功且签名守卫通过时 `set_value("")` 清缓冲，标记回归（此时凭据确实已存）；Clear 成功后 `credential_ref=None` → 标记关闭、标签回到“输入后显示星号”；保存失败（`committed=false`）整个同步块不执行，标志与缓冲均不动。标签"已保存”与标记同源于 `credential_ref`，一致。

## 三个风险面逐项核查

**1. 星号误当 Key——无。** 除上述分离外，关键是“删光重输再删光”场景：缓冲回到空时标记重现，语义上正确（凭据仍在、缓冲为空、下次提交是 Keep），标签与状态栏可区分，不算缺陷。固定 8 星不泄露真实长度，反而是隐私改进。

**2. 异步保存丢新编辑——无。** 复查了最危险的交错：提交 Replace 后继续在 key 字段输入——回执时签名失配，`set_value("")` 被 `!editing && revision 不变` 守卫拦住，新输入保留，状态追加“提交后的新修改尚未保存”；此时 `set_saved_secret(true)` 同步提交结果，但缓冲非空显示按字节星号，视觉不混淆。**关键顺序正确**：`unchanged` 先用 `f.signature(cx)` 计算，之后才 `set_saved_secret`，且后者不 bump revision——签名系统未被显示标志污染。`f.cfg` 在失配时只同步 `credential_ref` 形成局部混合态，但下次 `transact` 的 Keep 永远取自权威 `old`（循环侧 `cfg`），混合态仅影响标签显示，到不了存储。Clear 勾选的飞行中取消由 `flags[4]==f.clear` 守卫保留。

**3. 索引/IME/光标——安全。** 显示串 8 字节与空缓冲字节长不相等，但所有交互路径在空缓冲上短路：`index_for_mouse_position` 直接返回 0；光标/选区为 0，`x_for_index(0)` 有效；`character_index_for_point` 对空 content 的 `offset_to_utf16` 收敛为 0，无越界。组合输入一旦写入，content 非空即离开标记分支，回到字节等长的按字节星号路径；标记分支下 `marked_range` 必为空（或 0..0 被 `len>0` 过滤），`display_text.len()-marked_range.end` 无下溢。

## 附带核实

`settings.rs` 全量复核：Keep→old.ref 传递、Clear/Replace 失败保旧引用、staged 删除与复核、`api_key` 永不序列化、legacy Keep 升级 Replace 迁移——与 storage R2 关闭结论一致，本次未引入迁移类产品逻辑（对 public-model/loopback 的处置是数据级一次性修复，其日志未附，按声明采信）。

## 遗留（不阻塞）

两条备忘：删光输入后标记重现可能让用户误以为“删除已生效”，可考虑标签动态提示；`unchanged=false` 时 `f.cfg` 的局部 credential_ref 同步建议加注释说明仅显示用。

**判定：R6 有限 PASS；v1 共识保持历史，待新快照；G0–G5 未全通过，不批准发布。**