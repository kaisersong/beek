#!/bin/sh
# 组装并安装 BeeK.app —— 让 App 图标 / 菜单栏图标 / 悬浮按钮图标都来自 resources/icons/
#
# 三处图标各自的位置：
#   1) App 图标（Dock、Finder）: Contents/Resources/BeeK.icns + Info.plist 的 CFBundleIconFile
#   2) 菜单栏图标: 编译期内嵌 BeeKMenuBarTemplate@2x.png（src/icons.rs），setup_status_item 里 setImage
#   3) 悬浮按钮:   编译期内嵌 beek-selection-*-32@2x.png（src/icons.rs），IconView::render 里 img()
#
# 因此本脚本需要处理的是第 1 项：把 icns 放进 bundle 并声明 Info.plist 键。
set -e

ROOT=$(cd "$(dirname "$0")/.." && pwd)
APP=/Applications/BeeK.app
BIN_SRC="$ROOT/target/release/beek"
AGENT="$HOME/Library/LaunchAgents/com.kaisersong.beek.plist"

echo "== 1/5 构建 release =="
PATH="$HOME/.cargo/bin:$PATH" cargo build --release --manifest-path "$ROOT/Cargo.toml"
echo "构建产物：$(shasum -a 256 "$BIN_SRC" | cut -c1-16)"

echo "== 2/5 停掉旧进程（避免覆盖正在运行的二进制）=="
launchctl bootout "gui/$(id -u)/com.kaisersong.beek" 2>/dev/null || true
sleep 1
pkill -x BeeK 2>/dev/null || true
sleep 1
mkdir -p /tmp/beek_backup
if [ -f "$APP/Contents/MacOS/BeeK" ]; then
    cp "$APP/Contents/MacOS/BeeK" /tmp/beek_backup/BeeK
    cp "$APP/Contents/MacOS/BeeK" /tmp/beek_backup_binary
    echo "旧二进制已备份到 /tmp/beek_backup_binary"
fi
if [ -f "$APP/Contents/Info.plist" ]; then
    cp "$APP/Contents/Info.plist" /tmp/beek_backup/Info.plist
    echo "旧 Info.plist 已备份到 /tmp/beek_backup/Info.plist"
fi

echo "== 3/5 组装 bundle =="
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN_SRC" "$APP/Contents/MacOS/BeeK"
cp "$ROOT/resources/Info.plist" "$APP/Contents/Info.plist"
cp "$ROOT/resources/icons/BeeK.icns" "$APP/Contents/Resources/BeeK.icns"
echo "App 图标: $(ls -l "$APP/Contents/Resources/BeeK.icns" | awk '{print $5" 字节"}')"
echo "CFBundleIconFile: $(/usr/libexec/PlistBuddy -c Print:CFBundleIconFile "$APP/Contents/Info.plist")"

echo "== 4/5 ad-hoc 重签名 =="
codesign --force --sign - --identifier com.kaisersong.beek "$APP" 2>&1 | tail -2
codesign --verify --strict --verbose=2 "$APP" 2>&1 | tail -2 || echo "⚠️ 签名校验未通过"

echo "== 5/5 启动并刷新图标缓存 =="
launchctl bootstrap "gui/$(id -u)" "$AGENT"
sleep 3
pgrep -fl "$APP/Contents/MacOS/BeeK" || echo "⚠️ 进程未启动"
# LaunchServices / Dock 会缓存旧的（缺省）图标，touch bundle 让系统重新读 icns
touch "$APP"
killall Dock 2>/dev/null || true
echo "完成。日志：/tmp/beek.log"
