# 2026-10-17 晚间追加复测

资源恢复复测使用已冻结的003未签名安装包，build ID为 `69e09ecbc3a9ca689a5b613564e67c7e8c6cd25205936a6b318ceab864d71d49`，未改超时门槛或原始失败记录。随后应用户要求修复设置的已保存Key显示，另生成005；两批证据分别记录。

## 已确认

- 15:22 UTC，ilucas可用内存恢复至约17009MiB（总32602MiB），交互桌面session1已解锁。
- 首次尝试被Win32前台激活限制挡在驱动步骤，尚未执行产品断言。原始 `product-resource-recovery-cold.json` 保留，标为驱动前置阻塞，不能当作产品失败或成功。
- 公开记事本获得实际鼠标点击焦点后，同一最终Release完成真实取词→点击→正常流式→准确复制；源窗口前台和输入焦点保持。点击前服务请求为0，点击后为1。证据：`test-results/ilucas/product-resource-recovery-flow-v2.json`。
- 这次没有复现前一批helper超时。但资源恢复与复测成功不能单独证明低内存是原失败的唯一原因，原低内存失败仍保留待归因。

## 003资源恢复复测

003完成UIA与剪贴板预热、关闭BeeK可见窗口后，300个样本覆盖305.864秒：单核平均CPU 0.03576%，最高总工作集122.633MiB，最后进程数4（主进程+3个helper），目标≤1% / ≤150MiB，两项均通过。原始CSV及summary：`test-results/ilucas/idle-product-final-recovery.csv`和`.csv.summary.json`。这是003的暖机实测；旧失败/旧空闲样本仍保留，不作为005全量验收。

同包三轮冷启动脚本没有完成：新BeeK已启动，源记事本曾成功激活，但其他应用随后取得前台，未完成产品断言，实际完成轮数0。证据 `product-resource-recovery-cold-cycles.json` 保留为前台干扰阻塞，不记通过，也不能据此认定helper再次超时。

## 005默认模型及已保存Key修复

- 用户指出错误模型和空白Key外观。代码默认值本来与macOS一致；错误来自本轮测试残留的 `public-model` / loopback地址。一次性恢复 `doubao-seed-2-0-mini-260428` 和官方Ark完整地址；持有配置锁、校验并发变化，保留用户已保存的凭据引用与其他设置。
- 仅修改Windows `input.rs`、`ui.rs`：已有凭据显示固定八星和“已保存”标签。星号只参与绘制，不进入实际编辑缓冲；未编辑Key直接保存仍为Keep。异步保存继续保护提交后的新编辑。
- 新build ID：`eaa6216491a0f4efaa32daea37c6aa7e0e525abd99aeefa677fc0e523019053c`。Windows release构建、all-target测试22通过/1忽略、交互session1真实Vault及回滚测试23/23通过；macOS all-target check（dev-runtime-shaders）与Python工具22项通过。
- 005安装器退出0，已安装到ilucas。实际截图确认模型/地址正确、已保存标签及八星可见；未编辑Key直接保存前后凭据引用相同，交互session1确认Vault条目仍存在，重开设置仍显示星号。证据 `test-results/ilucas/saved-key-marker-gui.json`；截图已人工查看。没有读取真实Key内容到UI或日志，没有向真实API发送请求。
- 首次配置恢复因ReplaceFileW空备份参数封送失败而安全退出，文件未变；修正调用后成功。打包004在归档时失败，保留失败记录；005重新打包完整成功，不将004记成功。
- ZCode R6对星号与缓冲分离、异步编辑、索引/IME三处风险给出有限PASS；v2冻结与共识见[评审记录](reviews/implementation/consensus.md)。

新版内部预览在本地 `dist/windows-preview-20261017-key-state/`；未签名、不公开发布。

## 仍不代表完整发布通过

多屏/混合DPI/拔插、实际Explorer重启、代码签名、干净机、第二GPU、完整L1/故障/性能矩阵、8小时稳定性及完整macOS运行回归仍缺。源码共识不代替这些验收；未提交或发布。
