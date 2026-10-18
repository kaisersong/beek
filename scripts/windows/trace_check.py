#!/usr/bin/env python3
"""Assert G0 observable focus/click/tray invariants; never treats absence as PASS."""
import argparse
import json
from pathlib import Path
import sys


def check(events):
    errors = []
    required = {"started", "tray_ready", "lifetime_host", "overlay_native", "overlay_shown", "overlay_clicked", "heartbeat", "quit"}
    present = {e.get("event") for e in events}
    for name in sorted(required - present):
        errors.append(f"missing event: {name}")
    overlays = set()
    clicks = 0
    closed_since = None
    stayed_closed = False
    for event in events:
        name, data = event.get("event"), event.get("data", {})
        if name in ("overlay_error", "tray_error"):
            errors.append(f"native error: {name}")
        if name == "started" and (data.get("os") != "windows" or data.get("arch") != "x86_64"):
            errors.append("trace is not from Windows x64")
        if name == "overlay_native":
            overlays.add(data.get("hwnd"))
            style = data.get("extended_style", 0)
            if style & 0x08000080 != 0x08000080:
                errors.append("overlay lacks WS_EX_NOACTIVATE/TOOLWINDOW")
        if name == "lifetime_host" and data.get("visible") is not False:
            errors.append("lifecycle host must never be shown")
        if name == "overlay_shown" and (not data.get("foreground_before") or data.get("foreground_before") != data.get("foreground_after")):
            errors.append("overlay show changed foreground")
        if name == "overlay_clicked":
            clicks += 1
            if not data.get("foreground") or data.get("foreground") in overlays:
                errors.append("overlay click activated the popup")
        if name == "heartbeat":
            if data.get("lifetime_hosts") != 1 or data.get("gpui_windows") != data.get("visible_windows", -1) + 1:
                errors.append("heartbeat must account separately for the hidden lifecycle host")
            if data.get("visible_windows") == 0:
                if closed_since is None:
                    closed_since = event["ms"]
                stayed_closed |= event["ms"] - closed_since >= 30000
            else:
                closed_since = None
    if clicks < 10:
        errors.append("requires at least 10 actual overlay clicks")
    if not stayed_closed:
        errors.append("no evidence of at least 30 seconds alive with zero visible GPUI windows")
    # This checks structural invariants only. Visual alpha, source selection,
    # IME candidate placement and per-monitor correctness remain manual cases.
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    args = parser.parse_args()
    try:
        if args.log.stat().st_size > 10 * 1024 * 1024:
            print("NOT PASS: trace exceeds 10MiB")
            return 1
        events = [json.loads(line) for line in args.log.read_text(encoding="utf-8").splitlines() if line.strip()]
        errors = check(events)
    except (OSError, ValueError, KeyError, TypeError) as e:
        print(f"NOT PASS: invalid trace: {e}")
        return 1
    for error in errors:
        print(f"NOT PASS: {error}")
    if errors:
        return 1
    print("PASS: G0 trace invariants only; visual/IME acceptance remains separate.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
