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

echo "== 1/5 构建 + 安装（复用 package_app.sh，签名策略保持一致）=="
sh "$ROOT/scripts/package_app.sh"

echo "== 2/5 版本号写进 bundle（版本 $VERSION）=="
BUILD=$(/usr/libexec/PlistBuddy -c Print:CFBundleVersion "$ROOT/resources/Info.plist" 2>/dev/null || echo 1)
BUILD=$((BUILD + 1))
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$ROOT/resources/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD" "$ROOT/resources/Info.plist"
cp "$ROOT/resources/Info.plist" "$APP/Contents/Info.plist"
echo "   CFBundleShortVersionString=$(/usr/libexec/PlistBuddy -c Print:CFBundleShortVersionString "$APP/Contents/Info.plist") CFBundleVersion=$BUILD"

echo "== 3/5 重新签名（证书优先，保证辅助功能授权不丢）=="
DEVID=$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development: [^"]*\)".*/\1/p' | head -1)
if [ -n "$DEVID" ]; then
    codesign --force --sign "$DEVID" --identifier com.kaisersong.beek "$APP" 2>&1 | tail -1
else
    echo "⚠️ 没找到 Apple Development 证书，用 ad-hoc（用户需重新授权辅助功能）"
    codesign --force --sign - --identifier com.kaisersong.beek "$APP" 2>&1 | tail -1
fi
codesign --verify --strict "$APP" && echo "   签名校验通过"

echo "== 4/5 打包 zip（ditto 保留 bundle 元数据与签名）=="
mkdir -p "$ROOT/dist"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
echo "   $ZIP（$(ls -lh "$ZIP" | awk '{print $5}')）"
echo "   资产名约定：BeeK-v$VERSION.zip（app 的检查更新按这个名字找）"

echo "== 5/5 发布 =="
if [ "$1" != "--publish" ]; then
    echo "   跳过（要发布请加 --publish；发布前建议先在本地试一次 auto-update）"
    echo "   手动发布命令：gh release create v$VERSION \"$ZIP\" -R $REPO --title \"BeeK v$VERSION\""
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
gh release create "v$VERSION" "$ZIP" -R "$REPO" --title "BeeK v$VERSION" --notes-file "$NOTES"
echo "已发布：https://github.com/$REPO/releases/tag/v$VERSION"
