# Codex 实现自审 v2

结论：源码范围有限 PASS；G0–G5 未全部通过，不批准正式发布。

v1 后仅两个产品文件变化：`src/platform/windows/input.rs`、`src/platform/windows/ui.rs`。已核查固定八星仅用于绘制，真实编辑缓冲为空时提交 Keep，不会用星号替换凭据；保存成功同步标记，失败保留原状态；异步保存期间新编辑仍由原 revision 守卫保护；清除成功后取消标记。固定星号不暴露实际 Key 长度。

模型代码默认值原本正确，ilucas 上错误来自本轮残留的测试配置。已在配置锁和并发变化检查下，仅恢复测试 model/loopback endpoint，保留用户凭据引用及其他设置，没有读取真实 Key 内容或发起真实 API 请求。

Windows release 构建、session0 all-target 测试22通过/1忽略、交互 session1 包含真实 Vault 测试23/23通过；macOS all-target check（dev-runtime-shaders）和Python工具22项通过。安装器005退出0；实际设置截图确认默认模型、已保存标签及固定星号，直接保存前后凭据引用一致、Vault仍存在，重新打开仍显示。证据在本地 `test-results/ilucas/saved-key-marker-gui.json`。

冻结清单 SHA256 `e2b33455d39de93486afaf4897b25d2aef4934f1985880333d30a0e48ace7165`；完整源码指纹 `810987747069fb8d29d9effc8ced5c8b8bfddca946defa83c5e8eaee2c28013e`；build ID `eaa6216491a0f4efaa32daea37c6aa7e0e525abd99aeefa677fc0e523019053c`。v1 历史不可改写；G0 probe 补充清单继续使用 v1。ZCode R6 已有限 PASS，需绑定本次新清单。旧003的空闲/主流程复测仅代表旧二进制，不冒充本次完整87项验收。
