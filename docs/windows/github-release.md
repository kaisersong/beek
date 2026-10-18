# 双平台 CI 与下载

2026-10-18：根据用户要求，将 Windows 以未签名公开预览版加入 GitHub Releases；这项发布授权取代旧文档中仅允许内部预览的发布限制，不改变测试事实，也不表示完整 G0–G5 验收已完成。

- `Windows CI`：main 推送、PR、手动触发，Windows Server 2025 runner 上执行格式检查、Rust all-targets 测试、Python 工具测试、locked release 构建、配对二进制校验和 Inno Setup 打包。同时检查 macOS 编译与共享测试。
- `Release`：推送 `v*` tag 自动发布；手动 `publish=false` 仅留双平台 artifacts，`publish=true` 发布。版本取 Cargo.toml，tag 必须一致。
- macOS 保留 Developer ID 签名、公证以及 `BeeK.dmg` 稳定下载地址。Windows 提供版本化的 setup/ZIP/sha256，以及稳定的 `BeeK-windows-x64-setup.exe`、`BeeK-windows-x64.zip`。
- 两平台完成后聚合、校验资产与散列、上传草稿，最后才转公开。已有公开版本拒绝覆盖。稳定别名的散列另存 `SHA256SUMS.txt`。
- 默认 Windows manifest 标记 `public-preview-unsigned`；内部构建 `internal-preview-unsigned` 仍不能进入公开预览发布。配置签名 secrets 后可启用 `sign_windows` 或 `WINDOWS_SIGN_RELEASES=true`，并校验签名报告。
- CI 服务桌面不计为真实 UIA/焦点/输入法/DPI 桌面验收。真实 Windows 11 的手工/桌面测试和剩余硬件覆盖继续按完整测试方案记录。
