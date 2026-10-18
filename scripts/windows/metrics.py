#!/usr/bin/env python3
"""Report every selection attempt; never discard failures from p95/rate counts."""
import argparse
import csv
import json
import math
from pathlib import Path
import sys

APPS = ("notepad", "edge", "chrome")
OUTCOMES = {"success", "empty", "unsupported", "protected", "timeout", "busy", "stale", "error"}


def summarize(rows):
    groups = {}
    for row in rows:
        app, scenario = row["app"], row["scenario"]
        if scenario not in ("regular", "fault") or row["outcome"] not in OUTCOMES:
            raise ValueError("unknown scenario/outcome")
        elapsed = float(row["latency_ms"])
        if not math.isfinite(elapsed) or elapsed < 0:
            raise ValueError("invalid latency; failures must also have an elapsed time")
        group = groups.setdefault((app, scenario), {"latencies": [], "outcomes": {}, "loading": []})
        group["latencies"].append(elapsed)
        group["outcomes"][row["outcome"]] = group["outcomes"].get(row["outcome"], 0) + 1
        if row.get("loading_ms"):
            loading = float(row["loading_ms"])
            if not math.isfinite(loading) or loading < 0:
                raise ValueError("invalid local loading latency")
            group["loading"].append(loading)
    result = {}
    for (app, scenario), group in groups.items():
        latencies = sorted(group["latencies"])
        count = len(latencies)
        result[f"{app}/{scenario}"] = {
            "attempts": count, "outcomes": group["outcomes"],
            "success_rate": group["outcomes"].get("success", 0) / count,
            "timeout_rate": group["outcomes"].get("timeout", 0) / count,
            "p95_all_attempts_ms": latencies[math.ceil(count * .95) - 1],
            "max_all_attempts_ms": latencies[-1],
            "loading_samples": len(group["loading"]),
            "max_loading_ms": max(group["loading"], default=None),
        }
    return result


def acceptance_errors(result):
    errors = []
    for app in APPS:
        item = result.get(f"{app}/regular")
        if not item or item["attempts"] < 100:
            errors.append(f"{app}: need >=100 regular nonempty selection attempts")
            continue
        if item["success_rate"] != 1:
            errors.append(f"{app}: failed regular attempts require investigation, cannot hide them in p95")
        if item["p95_all_attempts_ms"] > 800:
            errors.append(f"{app}: selection-to-icon p95 exceeds 800ms")
        if item["loading_samples"] < 100 or item["max_loading_ms"] is None or item["max_loading_ms"] > 100:
            errors.append(f"{app}: need >=100 local loading measurements, each <=100ms")
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("csv", type=Path)
    parser.add_argument("--enforce", action="store_true")
    args = parser.parse_args()
    try:
        with args.csv.open(encoding="utf-8-sig", newline="") as file:
            result = summarize(csv.DictReader(file))
        print(json.dumps(result, ensure_ascii=False, indent=2))
        if args.enforce:
            errors = acceptance_errors(result)
            for error in errors:
                print(f"NOT PASS: {error}", file=sys.stderr)
            return 1 if errors else 0
        return 0
    except (OSError, ValueError, KeyError) as e:
        print(f"Invalid metrics: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
