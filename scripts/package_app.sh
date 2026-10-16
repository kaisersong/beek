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
AGENT="$HOME/Library/LaunchAgents/com.kaisersong.beek.plist"

echo "== 1/4 停掉旧进程（避免覆盖正在运行的二进制）=="
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

echo "== 2/4 构建 + 组装 bundle（与 CI 同一条组装路径）=="
sh "$ROOT/scripts/build_bundle.sh" /Applications

echo "== 3/4 重签名 =="
# 为什么优先用证书而不是默认的 ad-hoc：TCC 把「辅助功能」授权记在签名身份上，
# ad-hoc 身份每次重编译都会变 → 每次更新后取词都会失效，用户得再去系统设置里勾一次。
# 用稳定的 Apple Development 证书签名，授权就能跨更新保留。
DEVID=$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development: [^"]*\)".*/\1/p' | head -1)
if [ -n "$DEVID" ]; then
    echo "签名身份：$DEVID"
    codesign --force --sign "$DEVID" --identifier com.kaisersong.beek \
        --entitlements "$ROOT/resources/BeeK.entitlements" "$APP" 2>&1 | tail -2
else
    echo "⚠️ 没找到 Apple Development 证书，退回 ad-hoc："
    echo "   这会让「辅助功能」授权在本次更新后失效，需要在系统设置里重新勾选 BeeK。"
    codesign --force --sign - --identifier com.kaisersong.beek \
        --entitlements "$ROOT/resources/BeeK.entitlements" "$APP" 2>&1 | tail -2
fi
codesign --verify --strict --verbose=2 "$APP" 2>&1 | tail -2 || echo "⚠️ 签名校验未通过"

echo "== 4/4 启动并刷新图标缓存 =="
launchctl bootstrap "gui/$(id -u)" "$AGENT"
sleep 3
pgrep -fl "$APP/Contents/MacOS/BeeK" || echo "⚠️ 进程未启动"
# LaunchServices / Dock 会缓存旧的（缺省）图标，touch bundle 让系统重新读 icns
touch "$APP"
killall Dock 2>/dev/null || true
echo "完成。日志：/tmp/beek.log"
