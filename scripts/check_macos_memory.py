#!/usr/bin/env python3
"""Measure idle AX queries against a synthetic window, without reading user text.

Run `cargo test --bin beek` first. Requires macOS Accessibility permission.
Use --baseline-ref <revision> to compare a previous implementation.
Evidence remains in the printed temporary directory; all child processes stop.
"""
import argparse
import json
import os
from pathlib import Path
import plistlib
import re
import selectors
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
HARNESS = '''
#![allow(warnings)]
#[path="@AX@"] mod ax;
#[path="@ROOT@/src/mouse.rs"] mod mouse;
#[path="@ROOT@/src/eventtap.rs"] mod eventtap;
#[path="@ROOT@/src/macos_runtime.rs"] mod macos_runtime;
use std::io::{self, Write};
fn main() {
    assert!(unsafe { accessibility_sys::AXIsProcessTrusted() }, "AX permission required");
    let pid: i32 = std::env::args().nth(1).unwrap().parse().unwrap();
    for count in [100, 3000, 3000] {
        for _ in 0..count {
            let _pool = macos_runtime::AutoreleasePool::new();
            assert!(unsafe { ax::ax_selection_for_pid(false, None, Some(pid)) }.is_none());
            assert_ne!(unsafe { ax::focused_window_key(pid) }, 0);
            let _ = mouse::state();
        }
        println!("checkpoint {count}");
        io::stdout().flush().unwrap();
        let mut next = String::new();
        io::stdin().read_line(&mut next).unwrap();
    }
}
'''


def checked_output(command):
    return subprocess.check_output(command, cwd=ROOT, text=True, timeout=60)


def dependencies():
    deps = ROOT / "target/debug/deps"
    libraries = {}
    for name, version in [
        ("accessibility_sys", "accessibility-sys-0.2"),
        ("core_foundation", "core-foundation-0.9"),
        ("cocoa", "cocoa-0.25"),
        ("objc", "objc-0.2"),
    ]:
        for library in sorted(deps.glob(f"lib{name}-*.rlib")):
            manifest = library.with_name(library.name[3:]).with_suffix(".d")
            if manifest.exists() and version in manifest.read_text():
                libraries[name] = library
    assert len(libraries) == 4, "Run cargo test --bin beek first"
    return deps, libraries


def measure(label, ax_source, pid, work, deps, libraries):
    source = work / f"{label}.rs"
    source.write_text(HARNESS.replace("@AX@", str(ax_source)).replace("@ROOT@", str(ROOT)))
    executable = work / label
    command = ["rustc", "--edition=2021", "-Awarnings", str(source),
               "-L", f"dependency={deps}", "-o", str(executable)]
    for name, library in libraries.items():
        command += ["--extern", f"{name}={library}"]
    subprocess.run(command, cwd=ROOT, check=True, timeout=120)
    readings = []
    log_path = work / f"{label}.log"
    with log_path.open("w") as log:
        child = subprocess.Popen([str(executable), str(pid)], stdin=subprocess.PIPE,
                                 stdout=subprocess.PIPE, stderr=log, text=True)
        try:
            for step, count in enumerate([100, 3000, 3000]):
                with selectors.DefaultSelector() as selector:
                    selector.register(child.stdout, selectors.EVENT_READ)
                    assert selector.select(30), "AX query timed out"
                    checkpoint = child.stdout.readline().strip()
                assert checkpoint == f"checkpoint {count}", log_path.read_text()[-1500:]
                summary = checked_output(["vmmap", "-summary", str(child.pid)])
                (work / f"{label}-{step}.vmmap").write_text(summary)
                footprint = re.search(r"Physical footprint:\s+(\S+)", summary).group(1)
                malloc = re.search(r"DefaultMallocZone_.*", summary).group(0)
                reading = {"queries": count, "footprint": footprint, "malloc": malloc}
                readings.append(reading)
                print(label, reading, flush=True)
                child.stdin.write("next\n")
                child.stdin.flush()
            assert child.wait(timeout=5) == 0
        finally:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=5)
    return readings


def bytes_used(value):
    number, unit = re.fullmatch(r"([0-9.]+)([KMG])", value).groups()
    return float(number) * 1024 ** ("KMG".index(unit) + 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline-ref", help="Optional git revision, e.g. ae7713f")
    args = parser.parse_args()
    deps, libraries = dependencies()
    work = Path(tempfile.mkdtemp(prefix="beek-ax-pressure-"))
    implementations = []
    if args.baseline_ref:
        baseline = work / "baseline_ax.rs"
        baseline.write_text(checked_output(["git", "show", f"{args.baseline_ref}:src/ax.rs"]))
        implementations.append(("baseline", baseline))
    implementations.append(("fixed", ROOT / "src/ax.rs"))
    bundle = work / "Fixture.app"
    (bundle / "Contents/MacOS").mkdir(parents=True)
    (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps({
        "CFBundleExecutable": "fixture", "CFBundleIdentifier": "com.beek.memory-fixture",
        "CFBundleName": "BeeK memory fixture", "CFBundlePackageType": "APPL",
        "NSPrincipalClass": "NSApplication",
    }))
    subprocess.run(["clang", "-fobjc-arc", str(ROOT / "tests/macos/selection_fixture.m"),
                    "-framework", "Cocoa", "-o", str(bundle / "Contents/MacOS/fixture")],
                   check=True, timeout=60)
    ready = work / "fixture-ready.log"
    launcher = subprocess.Popen(["open", "-n", "-W", str(bundle), "--stdout", str(ready),
                                 "--stderr", str(work / "fixture.log")])
    pid = None
    try:
        for _ in range(100):
            if ready.exists() and ready.read_text().startswith("ready "):
                pid = int(ready.read_text().split()[1])
                break
            if launcher.poll() is not None:
                break
            time.sleep(0.1)
        assert pid is not None, f"Fixture did not launch; see {work}"
        results = {label: measure(label, source, pid, work, deps, libraries)
                   for label, source in implementations}
        fixed = results["fixed"]
        growth = bytes_used(fixed[-1]["footprint"]) - bytes_used(fixed[0]["footprint"])
        assert growth < 2 * 1024 * 1024, f"AX memory grew by {growth} bytes after warm-up"
        (work / "results.json").write_text(json.dumps(results, indent=2))
        print("evidence", work, flush=True)
    finally:
        if pid is not None:
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        launcher.wait(timeout=5)


if __name__ == "__main__":
    main()
