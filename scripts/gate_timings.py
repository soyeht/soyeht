#!/usr/bin/env python3
"""Per-step wall times of a gate run, kept in `.cache/gate-timings.json` (ADR 0089).

`scripts/check_all.sh` appends one tab-separated line per step (`id area seconds status`)
to a scratch file and calls `finalize` when it exits. The output file holds the latest
run in full and a short history of earlier runs, so slow steps can be watched over time.

    gate_timings.py finalize --steps FILE --tier TIER --exit-code N [--out PATH]
    gate_timings.py top [--out PATH] [--count N]
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUT = ROOT / ".cache/gate-timings.json"
HISTORY_LIMIT = 50


def read_steps(path: Path) -> list[dict]:
    steps = []
    for line in path.read_text().splitlines():
        fields = line.split("\t")
        if len(fields) != 4:
            continue
        step_id, area, seconds, status = fields
        steps.append({"id": step_id, "area": area, "seconds": float(seconds), "status": int(status)})
    return steps


def load(out: Path) -> dict:
    try:
        data = json.loads(out.read_text())
    except (OSError, ValueError):
        return {"history": []}
    return data if isinstance(data.get("history"), list) else {"history": []}


def finalize(steps: list[dict], tier: str, exit_code: int, areas: str, out: Path, now: float) -> dict:
    run = {
        "started_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(now)),
        "tier": tier,
        "areas": areas.split(),
        "exit_code": exit_code,
        "seconds": round(sum(step["seconds"] for step in steps), 2),
        "steps": steps,
    }
    data = load(out)
    summary = {key: run[key] for key in ("started_at", "tier", "exit_code", "seconds")}
    summary["steps"] = {step["id"]: step["seconds"] for step in steps}
    data = {"latest": run, "history": (data["history"] + [summary])[-HISTORY_LIMIT:]}
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(data, indent=2) + "\n")
    return data


def slowest(data: dict, count: int) -> list[tuple[str, float]]:
    """Mean seconds per step over the history, slowest first."""
    totals: dict[str, list[float]] = {}
    for run in data.get("history", []):
        for step_id, seconds in run.get("steps", {}).items():
            totals.setdefault(step_id, []).append(seconds)
    means = [(step_id, sum(values) / len(values)) for step_id, values in totals.items()]
    return sorted(means, key=lambda item: -item[1])[:count]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    final = commands.add_parser("finalize")
    final.add_argument("--steps", type=Path, required=True)
    final.add_argument("--tier", required=True)
    final.add_argument("--exit-code", type=int, required=True)
    final.add_argument("--areas", default="")
    top = commands.add_parser("top")
    top.add_argument("--count", type=int, default=10)
    for command in (final, top):
        command.add_argument("--out", type=Path, default=DEFAULT_OUT)
    arguments = parser.parse_args(argv)
    if arguments.command == "finalize":
        steps = read_steps(arguments.steps)
        finalize(steps, arguments.tier, arguments.exit_code, arguments.areas, arguments.out, time.time())
        total = sum(step["seconds"] for step in steps)
        print(f"check_all: {len(steps)} steps, {total:.0f}s; timings in {arguments.out}")
    else:
        for step_id, seconds in slowest(load(arguments.out), arguments.count):
            print(f"{seconds:8.1f}s  {step_id}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
