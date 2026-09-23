#!/usr/bin/env python3
"""Fail when measured code quality drifts from the versioned baseline."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from quality_core import (
    QualityError,
    differences,
    measure,
    periphery_status,
    tool_paths,
)


ROOT = Path(__file__).resolve().parents[1]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--print-baseline",
        action="store_true",
        help="print current measurements for a reviewed baseline update",
    )
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    args = parser.parse_args()
    try:
        root = args.root.resolve()
        jscpd, machete = tool_paths(root)
        current = measure(root, jscpd, machete)
        if args.print_baseline:
            print(json.dumps(current, indent=2, sort_keys=True))
            return 0
        expected = json.loads((root / "scripts/quality_baseline.json").read_text())
        if expected["version"] != current["version"]:
            raise QualityError("quality baseline version differs from the scanner")
        changes = differences(expected, current)
        for direction, explanation in changes:
            print(f"quality: {direction}: {explanation}", file=sys.stderr)
        print("quality: cargo-machete PASS")
        print(f"quality: Periphery {periphery_status(root)}")
        if changes:
            print(
                "quality: update scripts/quality_baseline.json in this PR for every "
                "change, including improvements. Baseline relaxations require written "
                "PR justification and reviewer approval.",
                file=sys.stderr,
            )
            return 1
        print(
            "quality: baseline matches; no new clones, oversized growth or loose literals"
        )
        return 0
    except (QualityError, OSError, KeyError, ValueError) as error:
        print(f"quality: FAIL: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
