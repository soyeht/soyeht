"""The flake ledger of the post-merge gate (ADR 0089).

A step that failed and then passed on the same commit (or could not be judged on a busy host) is
a flake: it blames nobody, so nothing else remembers it. `post_merge.py` appends every run to
`<health-dir>/flakes.json`, and `main_watch.py` reports in `watch.json` a step that flaked
`CHRONIC_FLAKES` times in the last `CHRONIC_WINDOW` runs as `chronic`: it is fixed, never skipped.

    {"runs":   [{"sha": ..., "date": ...}, ...],                  oldest first
     "flakes": [{"step": ..., "sha": ..., "load": {...} | null,
                 "date": ..., "kind": "passed-on-rerun" | "inconclusive"}, ...]}

This module reads and writes that file and chooses the steps a flaky run still has to run. It
runs nothing.
"""

from __future__ import annotations

import json
import os
from pathlib import Path

LEDGER_FILE = "flakes.json"
CHRONIC_WINDOW = 10  # runs
CHRONIC_FLAKES = 2  # flakes of one step in the window
RUN_HISTORY = 50  # runs the file keeps; older runs and their flakes go
PASSED_ON_RERUN, INCONCLUSIVE = "passed-on-rerun", "inconclusive"


def unrun_steps(areas: dict[str, str] | None, ran: list[dict]) -> list[str] | None:
    """The steps of the full tier that a run which stopped at its first failure never reached.

    `areas` lists the steps in the gate's order and `ran` is what the run recorded, including the
    step that failed. None when either is unknown: the run cannot say what it left out."""
    if not areas or not ran:
        return None
    seen = {step["id"] for step in ran}
    return [step for step in areas if step not in seen]


def flaky_step(step: str, record: dict, load: dict | None, kind: str = PASSED_ON_RERUN) -> dict:
    """The record's account of a flake: the load of the host it ran on when it was measured (a
    timing step), else the Mac's load at the end of the run."""
    return {"step": step, "kind": kind, "load": load or record.get("local_load")}


def read_ledger(health: Path) -> dict:
    """The ledger, with whatever is malformed in the file dropped: it is a record of hints, and a
    damaged one must not stop the gate from writing its verdict."""
    try:
        ledger = json.loads((health / LEDGER_FILE).read_text())
    except (OSError, ValueError):
        ledger = None
    if not isinstance(ledger, dict):
        ledger = {}
    runs, flakes = ledger.get("runs"), ledger.get("flakes")
    return {
        "runs": [run for run in runs if isinstance(run, dict) and isinstance(run.get("sha"), str)]
        if isinstance(runs, list) else [],
        "flakes": [flake for flake in flakes if isinstance(flake, dict)
                   and isinstance(flake.get("step"), str) and isinstance(flake.get("sha"), str)]
        if isinstance(flakes, list) else [],
    }


def record_run(health: Path, record: dict) -> None:
    """Add a finished run (and its flakes) to the ledger. A run of a commit already in it replaces
    that commit's entries (intentional: a `--force` rerun is the same run seen again, not a second
    flake). The file is replaced atomically."""
    sha, date = record["sha"], record["started_at"]
    ledger = read_ledger(health)
    runs = [run for run in ledger["runs"] if run["sha"] != sha] + [{"sha": sha, "date": date}]
    runs = runs[-RUN_HISTORY:]
    kept = {run["sha"] for run in runs}
    flakes = [flake for flake in ledger["flakes"] if flake["sha"] != sha and flake["sha"] in kept]
    flakes += [{"step": flake["step"], "sha": sha, "load": flake.get("load"), "date": date,
                "kind": flake["kind"]} for flake in record.get("flaky_steps", [])]
    health.mkdir(parents=True, exist_ok=True)
    temporary = health / f"{LEDGER_FILE}.tmp"
    temporary.write_text(json.dumps({"runs": runs, "flakes": flakes}, indent=2) + "\n")
    os.replace(temporary, health / LEDGER_FILE)


def chronic_steps(health: Path) -> list[dict]:
    """The steps with `CHRONIC_FLAKES` or more flakes in the last `CHRONIC_WINDOW` runs."""
    ledger = read_ledger(health)
    window = [run["sha"] for run in ledger["runs"][-CHRONIC_WINDOW:]]
    counts: dict[str, list[str]] = {}
    for flake in ledger["flakes"]:
        if flake["sha"] in window:
            counts.setdefault(flake["step"], []).append(flake["sha"])
    return [{"step": step, "flakes": len(shas), "runs": len(window), "shas": shas}
            for step, shas in sorted(counts.items()) if len(shas) >= CHRONIC_FLAKES]
