# BeeK 🐝

轻量 macOS AI 划词翻译工具 — Lightweight AI selection translator for macOS.

选中文本 → 小蜜蜂图标 → 点击翻译 → 弹窗显示。

## 特性

- **AX-only 取词**：不模拟按键，不读剪贴板
- **点击才翻译**：选中后显示小图标，点击后才请求 AI
- **豆包 Ark**：`doubao-seed-2-0-mini-260428`
- **GPUI UI**：Zed 同款 UI 框架，Ely 组件库

## 构建

```bash
# 需要 Rust stable
cargo build --release

# 设置 API Key (plist 或环境变量)
export BEEK_ARK_KEY="your-key"

# 运行
./target/release/beek
```

## 权限

系统设置 → 隐私与安全性 → 辅助功能 → 勾选 BeeK

**注意**：每次重新签名后需要重新授权。

## 架构

- `src/ax.rs` — AX 取词 (多层 fallback，支持 Safari/Chrome/Electron/原生)
- `src/translate.rs` — 豆包 Ark 翻译
- `src/main.rs` — GPUI 应用主入口
