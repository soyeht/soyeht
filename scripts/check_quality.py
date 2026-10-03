#!/usr/bin/env python3
"""Fail when measured code quality regresses past the versioned baseline."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from check_pane_interior import scan as pane_interior_problems
from quality_core import (
    QualityError,
    baseline_from_measurement,
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
            print(
                json.dumps(baseline_from_measurement(current), indent=2, sort_keys=True)
            )
            return 0
        expected = json.loads((root / "scripts/quality_baseline.json").read_text())
        if expected["version"] != current["version"]:
            raise QualityError("quality baseline version differs from the scanner")
        changes = differences(expected, current)
        regressions = [text for direction, text in changes if direction == "RELAXATION"]
        regressions += [
            f"pane interior {problem}; keep it flat (paneShell is the only raised surface)"
            for problem in pane_interior_problems(root)
        ]
        improvements = [text for direction, text in changes if direction == "IMPROVEMENT"]
        for explanation in regressions:
            print(f"quality: REGRESSION: {explanation}", file=sys.stderr)
        for explanation in improvements:
            print(f"quality: improvement: {explanation}")
        print("quality: cargo-machete PASS")
        print(f"quality: Periphery {periphery_status(root)}")
        if regressions:
            print(
                "quality: FAIL: fix each regression. Raising its entry in "
                "scripts/quality_baseline.json is a baseline relaxation that requires "
                "written PR justification and reviewer approval.",
                file=sys.stderr,
            )
            return 1
        if improvements:
            print(
                "quality: PASS with improvements; lower scripts/quality_baseline.json "
                "to the new values in this PR (python3 scripts/check_quality.py "
                "--print-baseline) so the ratchet keeps the gain"
            )
            return 0
        print(
            "quality: baseline matches; no new clones, oversized growth or loose literals"
        )
        return 0
    except (QualityError, OSError, KeyError, ValueError) as error:
        print(f"quality: FAIL: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
