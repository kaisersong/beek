# BeeK 🐝

轻量 macOS AI 划词翻译工具 — Lightweight AI selection translator for macOS.

选中文本 → 出现小蜜蜂图标 → 点击后翻译 → 弹窗显示结果。菜单栏常驻，由 LaunchAgent 托管。

## 特性

- **取词优先走 AX**：默认只通过 macOS 辅助功能接口读选中文本，多数应用（备忘录、Chrome、终端、访达、云之家等）选中即出图标，不用按键、不碰剪贴板。
- **AX 取不到时有两种兜底**（可在菜单里关）：「复制即弹按钮」= 你自己按 ⌘C，图标出现在鼠标旁；「剪贴板兜底取词」= BeeK 自动模拟一次 ⌘C 去取（读完全部还原剪贴板）。Safari、Muse、Grok Bot 这类不把选区交给 AX 的应用靠前者最稳。
- **点击才翻译**：出现图标时不请求；点击图标才发请求。
- **单个词给词典式详解**：选中的是单个词时，输出读音、词性与释义、例句、常见搭配。标题行有喇叭图标，点击用系统语音朗读该词。
- **弹窗可拖动**：按住卡片顶部拖动条可以移动弹窗。
- **点外部关闭**：在弹窗外面按一下鼠标就关闭弹窗。
- **Key 可配置**：菜单里配置；配置文件按服务（provider）分组，便于以后接第二个服务。

## 安装

### 方式 A：下载 Release

1. 打开 [Releases](https://github.com/kaisersong/beek/releases)，下载最新的 `BeeK-v<版本>.zip`（例如 `BeeK-v1.0.0.zip`）。
2. 解压，把 `BeeK.app` 拖进 `/Applications`。
3. 首次打开：在 Finder 里右键 `BeeK.app` → 「打开」，再确认一次。之后正常双击即可。

### 方式 B：本地构建

```bash
git clone https://github.com/kaisersong/beek.git
cd beek
sh scripts/package_app.sh
```

`scripts/package_app.sh` 按顺序做五件事：`cargo build --release`、停掉旧进程、组装 `/Applications/BeeK.app`、重新签名、启动 LaunchAgent。

需要 Rust stable 和 Xcode Command Line Tools。

## 获取 API Key

BeeK 通过火山引擎方舟（豆包 Ark）的 Chat Completions 接口翻译。使用前需要一个方舟 API Key。

1. 打开方舟控制台：<https://console.volcengine.com/ark>。
2. 完成实名认证。新账号需要这一步。
3. 在控制台左下角选择项目空间。这一步可选。
4. 进入「API Key 管理」，点「创建 API Key」。
5. 在弹框里确认或修改名称，点创建。
6. 在「API Key 管理」的列表里复制这个 Key。
7. 在「开通管理」里确认要用的模型已开通。默认模型是 `doubao-seed-2-0-mini-260428`。
8. 如果该模型要求推理接入点（Endpoint）：在控制台的「在线推理」页面创建接入点，把接入点 ID 填进配置的 `model` 字段。接入点 ID 可以直接当模型名用。

官方文档：[获取 API Key 并配置](https://www.volcengine.com/docs/82379/1541594)。

拿到 Key 后，点菜单栏的蜜蜂图标 → 「设置 API Key…」。菜单项「如何获取 API Key…」会打开本节的 GitHub 页面（锚点 `#获取-api-key`）。

## 配置

Key 有两种配置方式，配置文件优先：

1. 菜单「设置 API Key…」，写入 `~/Library/Application Support/BeeK/config.json`。**优先使用。**
2. 环境变量 `BEEK_ARK_KEY`。作为兜底。

`config.json` 完整示例：

```json
{
  "show_in_dock": false,
  "providers": {
    "ark": {
      "api_key": "your-ark-api-key",
      "model": "doubao-seed-2-0-mini-260428"
    }
  }
}
```

- `show_in_dock`：是否在 Dock 显示图标。菜单「在 Dock 中显示」改这个字段。
- `providers.ark.api_key`：方舟 API Key。
- `providers.ark.model`：可选。不填就用默认模型 `doubao-seed-2-0-mini-260428`。
- `providers.ark.base_url`：可选。不填就用方舟默认地址；填了就请求这个 OpenAI 兼容端点（例如自建网关）。

## 取词方式

BeeK 有三条取词路径，按可靠性依次尝试，前一条拿不到才走后面：

| 路径 | 触发 | 适用 | 代价 |
| --- | --- | --- | --- |
| AX 取词 | 选中文本即出图标 | 备忘录、Chrome、终端、访达、云之家等绝大多数原生/Chromium 应用 | 无（默认路径） |
| 复制即弹按钮 | 你自己按 ⌘C / 右键复制 | **任何应用**，包括 Safari、Muse、Grok Bot | 多按一次 ⌘C |
| 剪贴板兜底取词 | 划词手势后 AX 无结果 | AX 取不到、又不想按键时（时灵时不灵） | BeeK 会模拟一次 ⌘C，短暂占用剪贴板（读完全部还原） |

实测结论：Safari / Muse / Grok Bot 这类应用**不把选区暴露给 AX**（用 `BEEK_AX_PROBE=1 BEEK_AX_PROBE_PID=<pid>` 探针可以看到整棵 AX 树里 0 个节点有选中文本），所以它们只能靠后两条；其中「复制即弹按钮」是唯一 100% 可靠的。

## 权限

取词依赖辅助功能权限。

1. 系统设置 → 隐私与安全性 → 辅助功能 → 勾选 BeeK。
2. 也可以点菜单「开启取词权限（辅助功能）…」，直接跳到该面板。等价的命令：

```bash
open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
```

改完权限后如果取词没有反应，点菜单「重新启动 BeeK」。

**注意**：macOS 把辅助功能授权记在签名身份上。换一个身份签名，授权就失效，需要重新勾选一次。`scripts/package_app.sh` 优先用钥匙串里的 Apple Development 证书签名，授权因此能跨更新保留；找不到证书时它退回 ad-hoc 签名，这种情况下每次更新都要重新勾选。

## 更新

菜单「检查更新…」：

- 有新版本时，显示版本说明，可以点「下载并更新」。BeeK 下载 `BeeK-v<版本>.zip`，替换 `/Applications/BeeK.app`，然后重启。
- 已是最新时，只提示当前版本。

手动更新有两种方式：下载 zip 后替换 `/Applications/BeeK.app`；或者拉最新代码后运行 `sh scripts/package_app.sh`。

更新后如果取词失效，按上一节重新勾选辅助功能。

## 菜单

| 菜单项 | 作用 |
| --- | --- |
| BeeK 划词翻译 v1.0.0 | 显示版本，不可点击 |
| 开启取词权限（辅助功能）… | 打开系统的辅助功能面板 |
| 在 Dock 中显示 | 切换 Dock 图标，勾选项 |
| 设置 API Key… | 配置翻译服务的 Key 与模型 |
| ⓘ 如何获取 API Key… | 打开本仓库 README 的「获取 API Key」一节 |
| 复制即弹按钮（自己按 ⌘C） | 勾选项。你自己按 ⌘C 时，在鼠标旁弹出小蜜蜂 |
| 剪贴板兜底取词 | 勾选项。AX 取不到选区时，BeeK 自动模拟一次 ⌘C 去读选中文本 |
| 检查更新… | 检查新版本，可下载并更新 |
| 关于 BeeK | 显示版本信息，可跳 README 或 GitHub |
| 重新启动 BeeK | 重启进程。改完权限后用 |
| 退出 BeeK | 退出进程，同时移除 LaunchAgent |

版本号来自 `Cargo.toml` 的 `version`。

## 架构

- `src/main.rs` — GPUI 主入口：图标窗口、弹窗窗口、主循环
- `src/ax.rs` — AX 取词：多层回退读取选中文本与选区矩形
- `src/translate.rs` — 豆包 Ark 流式翻译；单词模式走词典式详解
- `src/menubar.rs` — 菜单栏状态项与菜单
- `src/config.rs` — 配置持久化
- `src/icons.rs` — 编译期内嵌图标素材
- `src/update.rs` — 检查更新、下载与替换 App
- `src/mouse.rs` — 全局鼠标状态，供拖动弹窗与「点外部关闭」使用

## 开发

自检开关都是环境变量：

| 变量 | 作用 |
| --- | --- |
| `BEEK_DEBUG=1` | 详细日志：AX 取值、流式响应、弹窗高度度量 |
| `BEEK_SELFTEST=1` | 注入模拟选区，跑完「取词 → 图标 → 弹窗」链路。不需要辅助功能权限 |
| `BEEK_TRANSLATE_SELFTEST=1` | 只量接口耗时，不开窗口 |

```bash
cargo build --release
BEEK_SELFTEST=1 BEEK_DEBUG=1 ./target/release/beek
```

运行日志在 `/tmp/beek.log`。

## 许可

MIT License，见 [LICENSE](LICENSE)。Copyright (c) 2026 Kaiser。
