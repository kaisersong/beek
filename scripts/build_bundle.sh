#!/bin/sh
# 只组装 BeeK.app —— 不安装、不启停进程、不签名。
#
#   本地：scripts/package_app.sh 会调它把 bundle 组装到 /Applications
#   CI：  scripts/build_bundle.sh dist 组装到 dist/BeeK.app，再由 CI 签名 + 公证 + 打包
#
# 为什么单独拆出来：签名和公证在 CI 上做，但「组装」两边必须一模一样，
# 否则会出现「本地能跑、CI 出来的包图标丢了 / 版本号不对」这种只能靠人比对的差异。
set -e

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT_DIR=${1:-$ROOT/dist}
APP="$OUT_DIR/BeeK.app"
BIN_SRC="$ROOT/target/release/beek"

VERSION=$(sed -n 's/^version = "\(.*\)".*/\1/p' "$ROOT/Cargo.toml" | head -1)
[ -n "$VERSION" ] || { echo "读不到 Cargo.toml 里的 version"; exit 1; }

echo "== 构建 release（版本 $VERSION）=="
PATH="$HOME/.cargo/bin:$PATH" cargo build --release --manifest-path "$ROOT/Cargo.toml"
echo "构建产物：$(shasum -a 256 "$BIN_SRC" | cut -c1-16)"

echo "== 组装 bundle → $APP =="
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN_SRC" "$APP/Contents/MacOS/BeeK"
cp "$ROOT/resources/Info.plist" "$APP/Contents/Info.plist"
cp "$ROOT/resources/icons/BeeK.icns" "$APP/Contents/Resources/BeeK.icns"

/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP/Contents/Info.plist"
# 构建号：CI 传 BEEK_BUILD（用 run number）覆盖；本地沿用 resources/Info.plist 里的值
if [ -n "${BEEK_BUILD:-}" ]; then
    /usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BEEK_BUILD" "$APP/Contents/Info.plist"
fi

echo "   CFBundleShortVersionString=$(/usr/libexec/PlistBuddy -c Print:CFBundleShortVersionString "$APP/Contents/Info.plist") CFBundleVersion=$(/usr/libexec/PlistBuddy -c Print:CFBundleVersion "$APP/Contents/Info.plist")"
echo "   CFBundleIconFile=$(/usr/libexec/PlistBuddy -c Print:CFBundleIconFile "$APP/Contents/Info.plist")（$(ls -l "$APP/Contents/Resources/BeeK.icns" | awk '{print $5" 字节"}')）"
