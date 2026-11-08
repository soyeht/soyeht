"""The shared runner of the ratchets that hold a count of sites in the engine down.

`check_event_literals.py` and `check_params_reads.py` each find their sites and give this module
what to say; the count may only go down, and the baseline follows it down in the same change.
"""

from __future__ import annotations

import argparse
import json
from collections.abc import Callable
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def run(
    *,
    description: str | None,
    label: str,
    key: str,
    baseline: Path,
    find: Callable[[Path], list[tuple[str, int]]],
    site: str,
    noun: str,
    fix: str,
    baseline_name: str,
) -> int:
    """Check the sites `find` returns against the baseline, or print the baseline."""
    parser = argparse.ArgumentParser(description=description)
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    parser.add_argument("--baseline", type=Path, default=baseline, help=argparse.SUPPRESS)
    parser.add_argument("--print-baseline", action="store_true")
    args = parser.parse_args()
    found = find(args.root.resolve())
    if args.print_baseline:
        print(json.dumps({key: len(found)}, indent=2))
        return 0
    allowed = json.loads(args.baseline.read_text(encoding="utf-8"))[key]
    if len(found) > allowed:
        for path, line in found:
            print(f"{label}: {path}:{line}: {site}")
        print(f"{label}: FAIL: {len(found)} {noun}, {allowed} allowed; {fix}")
        return 1
    if len(found) < allowed:
        print(
            f"{label}: FAIL: {len(found)} {noun}, baseline {allowed}; lower it with "
            f"--print-baseline > scripts/{baseline_name}"
        )
        return 1
    print(f"{label}: PASS: {len(found)} {noun}")
    return 0
