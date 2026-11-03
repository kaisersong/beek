# Embedded in the generated update script; arguments are set by update.rs.
set -eu
APP=$1
NEW=$2
VERSION=$3
RELAUNCH=$4
PARENT_PID=$5
LOG=$6
LABEL=com.kaisersong.beek
SERVICE="gui/$(id -u)/$LABEL"
AGENT="$HOME/Library/LaunchAgents/$LABEL.plist"
exec >>"$LOG" 2>&1
echo "== $(date '+%F %T') 开始替换：${VERSION} =="

# Finish copying and signing away from the installed path. Never overwrite a
# signed executable in place: its inode may still be mapped by a running app.
[ -f "$NEW/Contents/Info.plist" ] && [ -x "$NEW/Contents/MacOS/BeeK" ] || {
    echo "升级包不完整，保留旧版本"; exit 1;
}
WORK=$(mktemp -d "$(dirname "$APP")/.BeeK-update.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
INCOMING="$WORK/BeeK.app"
ditto "$NEW" "$INCOMING"
xattr -dr com.apple.quarantine "$INCOMING" 2>/dev/null || true
INCOMING_ID=$(codesign -dv --verbose=4 "$INCOMING" 2>&1 | sed -n 's/^Authority=\(Developer ID Application.*\)/\1/p' | head -1)
if [ -n "$INCOMING_ID" ]; then
    echo "保留 Developer ID 签名与公证：${INCOMING_ID}"
else
    DEVID=$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development: [^"]*\)".*/\1/p' | head -1)
    codesign --force --sign "${DEVID:--}" --identifier "$LABEL" "$INCOMING"
fi
codesign --verify --strict "$INCOMING"

# The updater has its own session, so bootout cannot kill it with the old job.
# Silent installs also wait for the caller to exit before switching bundles.
if [ "$RELAUNCH" = 1 ] && launchctl print "$SERVICE" >/dev/null 2>&1; then
    launchctl bootout "$SERVICE"
fi
i=0
while kill -0 "$PARENT_PID" 2>/dev/null && [ "$i" -lt 30 ]; do
    sleep 0.5
    i=$((i+1))
done
if kill -0 "$PARENT_PID" 2>/dev/null; then
    echo "旧进程仍在运行，放弃替换"; exit 1
fi

# Rename on the same volume; retain the old bundle until the switch succeeds.
if [ -e "$APP" ]; then mv "$APP" "$WORK/Previous.app"; fi
if ! mv "$INCOMING" "$APP"; then
    if [ -e "$WORK/Previous.app" ]; then
        mv "$WORK/Previous.app" "$APP" || {
            trap - EXIT
            echo "恢复失败，旧版本保留在 $WORK/Previous.app"; exit 1;
        }
    fi
    echo "安装失败，已恢复旧版本"; exit 1
fi

if [ "$RELAUNCH" = 1 ]; then
    # DMG/Finder installs have no LaunchAgent. Existing agents are optional;
    # failures must fall back to LaunchServices instead of leaving BeeK closed.
    if [ -f "$AGENT" ] && launchctl bootstrap "gui/$(id -u)" "$AGENT" && launchctl kickstart "$SERVICE"; then
        echo "已通过 LaunchAgent 启动新版本"
    else
        open "$APP"
        echo "已通过 LaunchServices 启动新版本"
    fi
else
    echo "静默安装完成，下次启动即为新版本"
fi
echo "== 替换完成：${VERSION} =="
