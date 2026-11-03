# BeeK 开发约定

- 默认中文。先核对当前代码和实际证据，区分本地测试、安装包、真实交互、CI 和公开 Release。
- `docs` 是指向 `../mydocs/beek` 的相对软链接，正文属于独立 mydocs 仓库。不得改回实体目录或把内部设计、评审、故障材料重新复制进代码仓库；单独检出代码时，构建与打包不得依赖 mydocs。
- 涉及模块边界、线程/进程、选区状态、配置、翻译或更新流程，先阅读 [系统设计](docs/design/system-design.md)，修改后同步对应章节；设想不能写成已实现行为。
- 涉及 AX/Core Foundation/Cocoa、Win32/COM、后台轮询、剪贴板、子进程或流式资源，必须阅读 [资源生命周期规则](docs/reliability/native-resource-lifecycle.md) 和 [2026-10-29 内存事件](docs/reliability/2026-10-29-macos-native-memory-leak.md)。文档未挂载时应明确缺失，不能声称已核对。
- CF Create/Copy 结果在 API 边界交给 owned `CFType`；Get/数组元素越过父作用域需要独立 retain。不能用 Get Rule 包装掩盖原 +1 泄漏，也不能对同一份 ownership 再手写第二次 release。
- 长驻 Cocoa 轮询每轮建立并清理自动释放池；池不替代 CF ownership，不跨线程、不跨 `.await`。提前返回、continue、失败和取消必须清理资源。
- 新后台任务必须声明频率、并发、队列/数据上限和终止条件。逻辑取消不等于真实线程/请求已退出。诊断模式必须有默认截止时间，不能留下无限运行的采集进程。
- 原生资源改动运行 `cargo test --locked --all-targets`，并在有权限的 macOS 桌面执行 `python3 scripts/check_macos_memory.py`；按改动补齐有选区、树搜索、菜单、图片剪贴板或异常场景。无桌面/无权限时记录具体未验证项，不用 Rust 单元测试代替真实内存压力证据。
- 不提交真实 Key、配置、用户选区、私人截图、进程环境或签名认证材料。现有日志可能含正文，共享前必须脱敏。
- 文档与代码分属两个仓库，提交时分别核对清单并保留无关 WIP。`docs` 迁移应同时包含旧实体文件的移除和软链接；只有用户要求提交/发布时才执行相应提交、推送、标签和发布操作。

- 公共翻译 Worker、固定出口中转、部署脚本、配置、凭据和备份只保存在 Git 仓库之外的私有运维目录，不得上传任何 GitHub 仓库；客户端仅保留非秘密的公开接口地址。
