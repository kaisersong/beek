#!/usr/bin/env python3
"""Versioned Windows acceptance records. A missing or stale check cannot PASS."""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import sys
import uuid

ROOT = Path(__file__).resolve().parents[2]
CASES = ROOT / "tests/windows/cases.json"
STAGES = tuple(f"G{i}" for i in range(6))
STATUSES = ("not_run", "pass", "fail", "blocked")
MEDIA = {".png", ".jpg", ".jpeg", ".mp4", ".webm", ".gif"}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def source_fingerprint(root: Path = ROOT) -> str:
    files = []
    for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "build.rs", "LICENSE", ".gitattributes",
                 "docs/windows/windows-port-plan.md", "docs/windows/windows-test-plan.md"):
        if (root / name).is_file():
            files.append(root / name)
    for folder in ("src", "tools/windows-probe", "scripts", "tests/windows", "resources", "packaging", ".cargo", ".github/workflows"):
        base = root / folder
        if not base.exists():
            continue
        for file in base.rglob("*"):
            relative = file.relative_to(base)
            if any(part in ("target", "__pycache__") for part in relative.parts):
                continue
            if file.is_file() and (file.suffix in (".rs", ".toml", ".lock", ".py", ".ps1", ".json", ".txt", ".html", ".sh", ".iss", ".rc", ".ico", ".png", ".icns", ".xml", ".plist", ".entitlements", ".yml", ".yaml") or file.name.startswith("GPUI-INPUT-")):
                if file.is_symlink():
                    raise ValueError(f"source symlink not permitted: {file}")
                files.append(file)
    value = hashlib.sha256()
    # pathlib sorts Windows paths case-insensitively. Explicit portable path
    # ordering makes an unchanged source bundle hash identically on both OSes.
    for file in sorted(set(files), key=lambda p: p.relative_to(root).as_posix()):
        value.update(file.relative_to(root).as_posix().encode())
        value.update(b"\0")
        value.update(file.read_bytes())
        value.update(b"\0")
    return value.hexdigest()


def load_cases() -> list[dict]:
    data = json.loads(CASES.read_text(encoding="utf-8"))
    if data.get("schema_version") != 1:
        raise ValueError("unsupported test catalog")
    seen = set()
    for case in data["cases"]:
        if case["id"] in seen or case["stage"] not in STAGES:
            raise ValueError("duplicate ID or unsupported stage")
        if not case["steps"] or not case["expected"]:
            raise ValueError("test missing executable steps/expectation")
        seen.add(case["id"])
    return data["cases"]


def save(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def evidence_path(run_dir: Path, relative: str) -> Path:
    path = (run_dir / relative).resolve()
    if not path.is_relative_to(run_dir.resolve()) or not path.is_file():
        raise ValueError(f"missing or escaping evidence: {relative}")
    return path


def environment_errors(env: dict) -> list[str]:
    errors = []
    if env.get("os") != "Windows":
        errors.append("requires an actual Windows host")
    if str(env.get("arch", "")).lower() not in ("amd64", "x86_64", "x64"):
        errors.append("requires x64, not ARM64 or emulation acceptance")
    try:
        if int(env.get("os_build", 0)) < 22000:
            errors.append("requires Windows 11 build >=22000")
    except (TypeError, ValueError):
        errors.append("invalid OS build")
    if env.get("interactive") is not True or env.get("session_id", 0) == 0:
        errors.append("requires a non-service interactive desktop session")
    for key in ("rustc", "cargo", "msvc", "windows_sdk", "gpu", "monitors"):
        if not env.get(key):
            errors.append(f"missing environment evidence: {key}")
    return errors


def create(out: Path, stage: str, environment: dict, artifact: Path | None, helper: Path | None = None, capabilities: dict | None = None) -> dict:
    if out.exists():
        raise ValueError("run already exists; create a new directory instead of overwriting evidence")
    cases = [c for c in load_cases() if c["stage"] == stage]
    try:
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True,
                                       stderr=subprocess.DEVNULL).strip()
    except (OSError, subprocess.CalledProcessError):
        head = "unavailable"
    def copy_artifact(file):
        if file is None:
            return None
        destination = out.parent / "artifacts" / file.name
        destination.parent.mkdir(parents=True, exist_ok=True)
        if destination.resolve() != file.resolve():
            destination.write_bytes(file.read_bytes())
        return {"path": destination.relative_to(out.parent).as_posix(), "sha256": digest(destination)}
    artifact_info = copy_artifact(artifact)
    helper_info = copy_artifact(helper)
    report = {
        "schema_version": 1, "run_id": str(uuid.uuid4()),
        "created_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "stage": stage, "source_head": head, "source_fingerprint": source_fingerprint(),
        "catalog_sha256": digest(CASES), "environment": environment,
        "artifact": artifact_info, "helper_artifact": helper_info, "binary_capabilities": capabilities,
        "cases": [{"id": c["id"], "status": "not_run", "actual": "", "evidence": []} for c in cases],
    }
    if out.exists():
        raise ValueError("run already exists; create a new directory instead of overwriting evidence")
    save(out, report)
    return report


def validate(report: dict, run_dir: Path, current: str | None = None) -> list[str]:
    errors = []
    if report.get("schema_version") != 1:
        return ["unsupported report schema"]
    if report.get("stage") not in STAGES:
        return ["unknown stage"]
    errors.extend(environment_errors(report.get("environment", {})))
    if report["stage"] in ("G2", "G3", "G4", "G5"):
        apps = report.get("environment", {}).get("apps", {})
        for app in ("notepad", "edge", "chrome"):
            if not apps.get(app):
                errors.append(f"missing tested application version: {app}")
    if report.get("source_fingerprint") != (current or source_fingerprint()):
        errors.append("stale source fingerprint; rerun acceptance against current code")
    if report.get("catalog_sha256") != digest(CASES):
        errors.append("stale test catalog")
    catalog = {c["id"]: c for c in load_cases() if c["stage"] == report["stage"]}
    seen = set()
    for case in report.get("cases", []):
        identifier = case.get("id")
        if identifier in seen or identifier not in catalog:
            errors.append(f"duplicate or unknown test: {identifier}")
            continue
        seen.add(identifier)
        if case.get("status") != "pass":
            errors.append(f"{identifier}: {case.get('status', 'missing')} (not PASS)")
            continue
        if not case.get("actual", "").strip():
            errors.append(f"{identifier}: missing actual result")
        evidence = case.get("evidence", [])
        if not evidence:
            errors.append(f"{identifier}: missing evidence files")
        media = False
        for item in evidence:
            try:
                path = evidence_path(run_dir, item["path"])
                if digest(path) != item.get("sha256"):
                    errors.append(f"{identifier}: changed evidence {item['path']}")
                if path.stat().st_size == 0:
                    errors.append(f"{identifier}: empty evidence {item['path']}")
                media |= path.suffix.lower() in MEDIA
            except (ValueError, KeyError) as e:
                errors.append(f"{identifier}: {e}")
        if catalog[identifier]["method"] == "manual" and not media:
            errors.append(f"{identifier}: manual desktop check requires screenshot/video")
    for missing in sorted(catalog.keys() - seen):
        errors.append(f"missing required case: {missing}")
    artifact = report.get("artifact")
    if not artifact:
        errors.append("missing tested binary and SHA256")
    else:
        try:
            expected = "beek-windows-probe.exe" if report["stage"] == "G0" else "beek.exe"
            if Path(artifact["path"]).name.lower() != expected:
                errors.append(f"wrong tested binary for {report['stage']}: expected {expected}")
            if digest(evidence_path(run_dir, artifact["path"])) != artifact.get("sha256"):
                errors.append("tested binary changed")
        except (KeyError, ValueError) as e:
            errors.append(str(e))
    if report["stage"] != "G0":
        caps=report.get("binary_capabilities") or {}
        main, host = caps.get("main",{}), caps.get("selection-host",{})
        if not main or not host:errors.append("missing tested main/helper capabilities")
        if main.get("build_id")!=host.get("build_id") or not main.get("build_id"):errors.append("main/helper build mismatch")
        for kind, info, artifact_key in (("main",main,"artifact"),("selection-host",host,"helper_artifact")):
            if info.get("binary")!=kind or info.get("os")!="windows" or info.get("arch")!="x86_64" or info.get("native_machine")!=34404 or info.get("protocol")!=1:errors.append("invalid tested binary capability: "+kind)
            if info.get("test_fixtures") is not False or info.get("debug_assertions") is not False:errors.append("fixture/debug binary cannot pass release acceptance: "+kind)
            if info.get("sha256")!=(report.get(artifact_key) or {}).get("sha256"):errors.append("capability binary hash mismatch: "+kind)
        helper = report.get("helper_artifact")
        try:
            if not helper or Path(helper["path"]).name.lower() != "beek-selection-host.exe":
                errors.append("missing tested selection helper binary")
            elif digest(evidence_path(run_dir, helper["path"])) != helper.get("sha256"):
                errors.append("tested helper binary changed")
        except (KeyError, ValueError) as e:
            errors.append(str(e))
    if report["stage"] == "G0" and not (ROOT / "tools/windows-probe/Cargo.lock").exists():
        errors.append("G0 dependency lockfile missing")
    return errors


def release_gate(runs: list[Path]) -> list[str]:
    errors, seen = [], set()
    for run in runs:
        report = json.loads(run.read_text(encoding="utf-8"))
        stage = report.get("stage")
        if stage in seen:
            errors.append(f"duplicate stage: {stage}")
        seen.add(stage)
        errors.extend(f"{stage}: {e}" for e in validate(report, run.parent))
    for missing in sorted(set(STAGES) - seen):
        errors.append(f"missing required gate: {missing}")
    return errors


def record_case(run: Path, identifier: str, status: str, actual: str, files: list[str]) -> None:
    if status not in STATUSES:
        raise ValueError("unknown status")
    report = json.loads(run.read_text(encoding="utf-8"))
    case = next((c for c in report["cases"] if c["id"] == identifier), None)
    if case is None:
        raise ValueError("case not part of this run")
    if status == "pass" and (not actual.strip() or not files):
        raise ValueError("PASS requires an actual result and evidence files")
    entries = []
    for file in files:
        path = evidence_path(run.parent, file)
        entries.append({"path": path.relative_to(run.parent.resolve()).as_posix(), "sha256": digest(path)})
    case.update(status=status, actual=actual, evidence=entries,
                recorded_utc=dt.datetime.now(dt.timezone.utc).isoformat())
    save(run, report)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    new = commands.add_parser("create")
    new.add_argument("--stage", choices=STAGES, required=True)
    new.add_argument("--out", type=Path, required=True)
    new.add_argument("--environment", type=Path)
    new.add_argument("--artifact", type=Path)
    new.add_argument("--helper-artifact", type=Path)
    new.add_argument("--capabilities",type=Path)
    record = commands.add_parser("record")
    record.add_argument("--run", type=Path, required=True)
    record.add_argument("--case", required=True)
    record.add_argument("--status", choices=STATUSES, required=True)
    record.add_argument("--actual", required=True)
    record.add_argument("--evidence", action="append", default=[])
    check = commands.add_parser("validate")
    check.add_argument("--run", type=Path, required=True)
    release = commands.add_parser("release-gate")
    release.add_argument("--run", action="append", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "create":
            env = json.loads(args.environment.read_text(encoding="utf-8-sig")) if args.environment else {
                "os": platform.system(), "arch": platform.machine(), "interactive": False,
            }
            caps=json.loads(args.capabilities.read_text(encoding="utf-8-sig")) if args.capabilities else None
            report = create(args.out.resolve(), args.stage, env, args.artifact, args.helper_artifact,caps)
            print(f"Created {report['run_id']}; all {len(report['cases'])} cases NOT RUN.")
        elif args.command == "record":
            record_case(args.run.resolve(), args.case, args.status, args.actual, args.evidence)
        elif args.command == "validate":
            report = json.loads(args.run.read_text(encoding="utf-8"))
            errors = validate(report, args.run.parent)
            for error in errors:
                print(f"NOT PASS: {error}")
            if errors:
                return 1
            print(f"PASS: {report['stage']} on source {report['source_fingerprint']}")
        else:
            errors = release_gate(args.run)
            for error in errors:
                print(f"NOT PASS: {error}")
            if errors:
                return 1
            print("PASS: all G0-G5 evidence gates; publication still requires signing and release workflow checks.")
        return 0
    except (OSError, ValueError, KeyError) as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
