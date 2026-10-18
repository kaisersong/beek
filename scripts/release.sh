#!/bin/sh
# 打包并（可选）发布新版本
#
#   只打包：  sh scripts/release.sh
#   打包+发布：sh scripts/release.sh --publish
#
# 产物：dist/BeeK-v<版本>.zip —— 内含已签名的 BeeK.app，
# app 里的「检查更新…」就是靠这个资产名（BeeK-v<版本>.zip）找到升级包的。
#
# 版本号只有一个来源：Cargo.toml 的 version。脚本会把它写进 Info.plist，
# 这样「关于」窗口、Finder 显示、更新比较三处一定一致。
set -e

ROOT=$(cd "$(dirname "$0")/.." && pwd)
REPO="kaisersong/beek"
APP=/Applications/BeeK.app
AGENT="$HOME/Library/LaunchAgents/com.kaisersong.beek.plist"

VERSION=$(sed -n 's/^version = "\(.*\)".*/\1/p' "$ROOT/Cargo.toml" | head -1)
[ -n "$VERSION" ] || { echo "读不到 Cargo.toml 里的 version"; exit 1; }
ZIP="$ROOT/dist/BeeK-v$VERSION.zip"

# 版本号必须在 package_app.sh **之前**写：那个脚本会把这份 Info.plist 装进 bundle、然后启动 app。
# 反过来（先启动、再改 plist）会让进程里的 NSBundle 缓存住旧版本，于是出现
# 「菜单栏显示新版本、关于窗口显示旧版本」的不一致 —— 1.0.5 就是这么翻的。
echo "== 1/6 版本号写进 resources/Info.plist（版本 ${VERSION}）=="
BUILD=$(/usr/libexec/PlistBuddy -c Print:CFBundleVersion "$ROOT/resources/Info.plist" 2>/dev/null || echo 1)
BUILD=$((BUILD + 1))
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$ROOT/resources/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD" "$ROOT/resources/Info.plist"
echo "   resources/Info.plist：CFBundleShortVersionString=$VERSION CFBundleVersion=$BUILD"

echo "== 2/6 构建 + 安装（复用 package_app.sh，签名策略保持一致）=="
sh "$ROOT/scripts/package_app.sh"
echo "   已安装 bundle：$(/usr/libexec/PlistBuddy -c Print:CFBundleShortVersionString "$APP/Contents/Info.plist")（构建号 $(/usr/libexec/PlistBuddy -c Print:CFBundleVersion "$APP/Contents/Info.plist")）"

echo "== 3/6 重新签名（证书优先，保证辅助功能授权不丢）=="
DEVID=$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development: [^"]*\)".*/\1/p' | head -1)
if [ -n "$DEVID" ]; then
    codesign --force --sign "$DEVID" --identifier com.kaisersong.beek "$APP" 2>&1 | tail -1
else
    echo "⚠️ 没找到 Apple Development 证书，用 ad-hoc（用户需重新授权辅助功能）"
    codesign --force --sign - --identifier com.kaisersong.beek "$APP" 2>&1 | tail -1
fi
codesign --verify --strict "$APP" && echo "   签名校验通过"

echo "== 4/6 打包 zip（ditto 保留 bundle 元数据与签名）=="
mkdir -p "$ROOT/dist"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "   ${ZIP}（$(ls -lh "$ZIP" | awk '{print $5}')）"
echo "   资产名约定：BeeK-v$VERSION.zip（app 的检查更新按这个名字找）"

echo "== 5/6 冒烟：点 Dock 图标（重新打开）不能崩 =="
# 这条是踩过坑才加的：曾经在 gpui 持有 App 借用时同步弹模态菜单，
# 点一次 Dock 图标崩一次（KeepAlive 拉起来再崩）。用 open -a 复现，不用手点。
# 判据：pid 不变 + 崩溃报告数不增。
CRASH_DIR="$HOME/Library/Logs/DiagnosticReports"
pid_before=$(pgrep -f "$APP/Contents/MacOS/BeeK" | head -1)
crashes_before=$(ls "$CRASH_DIR" 2>/dev/null | grep -c '^BeeK-' || true)
open -a "$APP"
sleep 5
pid_after=$(pgrep -f "$APP/Contents/MacOS/BeeK" | head -1)
crashes_after=$(ls "$CRASH_DIR" 2>/dev/null | grep -c '^BeeK-' || true)
echo "   pid $pid_before → ${pid_after}；崩溃报告 $crashes_before → $crashes_after"
if [ "$pid_before" != "$pid_after" ] || [ "$crashes_after" != "$crashes_before" ]; then
    echo "❌ 触发「重新打开」后进程重启或新增崩溃报告 —— 不发布"
    exit 1
fi
echo "   ✅ 没崩（弹出的是菜单栏那份菜单，是模态的）"
# 模态菜单会挂在屏幕上，重启进程把它收掉
launchctl kickstart -k "gui/$(id -u)/com.kaisersong.beek" >/dev/null 2>&1 || true
sleep 2

echo "== 6/6 发布 =="
if [ "$1" != "--publish" ]; then
    echo "   跳过（要发布请加 --publish；发布前建议先在本地试一次 auto-update）"
    echo "   正式发布请使用双平台 Release 工作流。"
    exit 0
fi

# 说明文本：优先用 dist/RELEASE_NOTES.md，否则用上一个 tag 之后的提交
NOTES="$ROOT/dist/RELEASE_NOTES.md"
if [ ! -f "$NOTES" ]; then
    LAST=$(git -C "$ROOT" describe --tags --abbrev=0 2>/dev/null || true)
    {
        echo "## 变更"
        if [ -n "$LAST" ]; then
            git -C "$ROOT" log --pretty=format:'- %s' "$LAST..HEAD"
        else
            git -C "$ROOT" log --pretty=format:'- %s' -20
        fi
        printf '\n\n## 安装\n下载 `BeeK-v%s.zip`，解压后把 BeeK.app 拖进「应用程序」。\n' "$VERSION"
    } > "$NOTES"
fi
echo "正式发布统一由 GitHub Release 工作流聚合 Windows 安装包/便携包 + macOS 签名公证包。"
echo "本地 --publish 不再绕过双平台资产校验；请使用 Actions → Release。"
exit 1
