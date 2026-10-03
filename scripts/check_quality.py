#!/usr/bin/env python3
"""Fail unless measured code quality matches the versioned baseline exactly."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from check_pane_interior import scan as pane_interior_problems
from gate_base import BASE_BRANCH, resolve_base
from quality_core import (
    BASELINE_NAME,
    JUSTIFICATIONS_NAME,
    PANIC_CEILING,
    PANIC_KEY,
    QualityError,
    baseline_at,
    baseline_from_measurement,
    differences,
    load_justifications,
    measure,
    periphery_status,
    tool_paths,
    unjustified_relaxations,
)


ROOT = Path(__file__).resolve().parents[1]
REFRESH_COMMAND = (
    f"uv run python scripts/check_quality.py --print-baseline > {BASELINE_NAME}"
)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--print-baseline",
        action="store_true",
        help="print current measurements for a reviewed baseline update",
    )
    parser.add_argument(
        "--base",
        help=f"full base commit OID (default: merge-base of {BASE_BRANCH} and HEAD)",
    )
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    args = parser.parse_args()
    try:
        root = args.root.resolve()
        base = None if args.print_baseline else resolve_base(root, args.base)
        jscpd, machete = tool_paths(root)
        current = measure(root, jscpd, machete)
        if args.print_baseline:
            print(
                json.dumps(baseline_from_measurement(current), indent=2, sort_keys=True)
            )
            return 0
        expected = json.loads((root / BASELINE_NAME).read_text())
        if expected["version"] != current["version"]:
            raise QualityError("quality baseline version differs from the scanner")
        changes = differences(expected, current)
        regressions = [c.message for c in changes if c.direction == "RELAXATION"]
        if expected.get(PANIC_KEY) != PANIC_CEILING:
            regressions.append(
                f"{PANIC_KEY} is absolute and must stay {PANIC_CEILING} in {BASELINE_NAME}"
            )
        regressions += [
            f"pane interior {problem}; keep it flat (paneShell is the only raised surface)"
            for problem in pane_interior_problems(root)
        ]
        unlocked = [c.message for c in changes if c.direction == "IMPROVEMENT"]
        unjustified = unjustified_relaxations(
            baseline_at(root, base), expected, load_justifications(root)
        )
        for explanation in regressions:
            print(f"quality: REGRESSION: {explanation}", file=sys.stderr)
        for explanation in unlocked:
            print(f"quality: UNLOCKED IMPROVEMENT: {explanation}", file=sys.stderr)
        for change in unjustified:
            print(
                f"quality: UNJUSTIFIED BASELINE RAISE vs {base[:12]}: {change.message}",
                file=sys.stderr,
            )
        print("quality: cargo-machete PASS")
        print(f"quality: Periphery {periphery_status(root)}")
        failed = False
        if regressions:
            print(
                "quality: FAIL: fix each regression. Raising its baseline entry is a "
                f"relaxation that needs a written reason in {JUSTIFICATIONS_NAME}.",
                file=sys.stderr,
            )
            failed = True
        if unlocked:
            print(
                "quality: FAIL: the baseline still allows what this change removed; "
                f"lock the gain in this PR: {REFRESH_COMMAND}",
                file=sys.stderr,
            )
            failed = True
        if unjustified:
            template = {
                change.key: {"value": change.current, "reason": "why this must grow"}
                for change in unjustified
            }
            print(
                "quality: FAIL: each raised baseline entry needs a reason in "
                f"{JUSTIFICATIONS_NAME}, for example:\n"
                + json.dumps(template, indent=2),
                file=sys.stderr,
            )
            failed = True
        if failed:
            return 1
        print(
            "quality: baseline matches exactly; no new clones, oversized growth, "
            "loose literals or production panics"
        )
        return 0
    except (QualityError, OSError, KeyError, ValueError) as error:
        print(f"quality: FAIL: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
