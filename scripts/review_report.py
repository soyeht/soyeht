#!/usr/bin/env python3
"""Print the Q1 quality delta against a PR base commit."""

from __future__ import annotations

import argparse
import io
import json
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

from quality_core import (
    AREAS,
    BASELINE_NAME,
    DUPLICATION_SCANS,
    JUSTIFICATIONS_NAME,
    PANIC_KEY,
    QualityError,
    SWIFT_LITERAL_PATTERNS,
    baseline_at,
    baseline_from_measurement,
    baseline_relaxations,
    differences,
    justification_for,
    load_justifications,
    measure,
    periphery_status,
    tool_paths,
    unjustified_relaxations,
)


ROOT = Path(__file__).resolve().parents[1]


def base_measurement(commit: str, jscpd: Path, machete: str) -> dict:
    # Scan the base even when it has a baseline: percent is report-only.
    archive = subprocess.run(
        [
            "git",
            "archive",
            "--format=tar",
            commit,
            "engine",
            "apps/shared",
            "apps/mac/Sources",
            "apps/ios/Sources",
        ],
        cwd=ROOT,
        capture_output=True,
        check=True,
    )
    with tempfile.TemporaryDirectory(prefix="soyeht-quality-base-") as directory:
        with tarfile.open(fileobj=io.BytesIO(archive.stdout)) as tar:
            tar.extractall(directory, filter="data")
        return measure(Path(directory), jscpd, machete)


def base_baseline(commit: str, measured: dict) -> dict:
    existing = baseline_at(ROOT, commit)
    return existing if existing is not None else baseline_from_measurement(measured)


def literal_totals(measurement: dict, area: str) -> dict[str, int]:
    prefix = AREAS[area][0] + "/"
    kinds = ("rust_limits",) if area == "engine" else SWIFT_LITERAL_PATTERNS
    return {
        kind: sum(
            counts.get(kind, 0)
            for path, counts in measurement["loose_literals"].items()
            if path.startswith(prefix)
        )
        for kind in kinds
    }


def render(
    before: dict,
    after: dict,
    current: dict,
    base: str,
    periphery: str,
    previous_baseline: dict | None = None,
    justifications: dict[str, dict] | None = None,
) -> str:
    lines = [f"Q1 quality delta vs {base}"]
    for area in DUPLICATION_SCANS:
        old = before["duplication"][area]
        new = current["duplication"][area]
        lines.append(
            f"duplication {area}: {old['percent']}% -> {new['percent']}%; "
            f"clones {old['clones']} -> {new['clones']}; "
            f"lines {old['duplicated_lines']} -> {new['duplicated_lines']}"
        )
    old_files = before["oversized_files"]
    new_files = current["oversized_files"]
    lines.append(f">800-line exceptions: {len(old_files)} -> {len(new_files)}")
    for path in sorted(old_files.keys() | new_files.keys()):
        if old_files.get(path) != new_files.get(path):
            lines.append(
                f"  {path}: {old_files.get(path, 'none')} -> {new_files.get(path, 'none')}"
            )
    for area in AREAS:
        previous = literal_totals(before, area)
        for name, value in literal_totals(current, area).items():
            old = previous[name]
            lines.append(f"literal {area}.{name}: {old} -> {value}")
    old_literals = before["loose_literals"]
    new_literals = current["loose_literals"]
    for path in sorted(old_literals.keys() | new_literals.keys()):
        old = old_literals.get(path, {})
        new = new_literals.get(path, {})
        if old != new:
            lines.append(f"  literal {path}: {old or 'none'} -> {new or 'none'}")
    lines.append(
        f"engine production unwrap/expect/panic!: {before[PANIC_KEY]} -> "
        f"{current[PANIC_KEY]}"
    )
    lines.append("cargo-machete: PASS")
    lines.append(f"Periphery: {periphery}")
    relaxations = baseline_relaxations(
        previous_baseline or baseline_from_measurement(before), after
    )
    if relaxations:
        lines.append(
            "🔴 BASELINE RELAXATION — written justification required in "
            f"{JUSTIFICATIONS_NAME}:"
        )
        for change in relaxations:
            reason = justification_for(change, justifications or {})
            lines.append(
                f"🔴 {change.message} — "
                + (f"justification: {reason}" if reason else "MISSING justification")
            )
    else:
        lines.append("Baseline relaxations: none")
    drift = differences(after, current)
    regressions = [c.message for c in drift if c.direction == "RELAXATION"]
    improvements = [c.message for c in drift if c.direction == "IMPROVEMENT"]
    if regressions:
        lines.append("FAIL: current measurements regress past the checked-in baseline:")
        lines.extend(f"  {explanation}" for explanation in regressions)
    if improvements:
        lines.append("FAIL: improvements not yet recorded; lower the baseline in this PR:")
        lines.extend(f"  {explanation}" for explanation in improvements)
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", help="PR base revision or full commit OID")
    args = parser.parse_args()
    try:
        commit = subprocess.run(
            ["git", "rev-parse", "--verify", f"{args.base}^{{commit}}"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        jscpd, machete = tool_paths(ROOT)
        before = base_measurement(commit, jscpd, machete)
        previous_baseline = base_baseline(commit, before)
        after = json.loads((ROOT / BASELINE_NAME).read_text())
        current = measure(ROOT, jscpd, machete)
        justifications = load_justifications(ROOT)
        print(
            render(
                before,
                after,
                current,
                commit[:12],
                periphery_status(ROOT),
                previous_baseline,
                justifications,
            )
        )
        failed = bool(differences(after, current)) or bool(
            unjustified_relaxations(previous_baseline, after, justifications)
        )
        return 1 if failed else 0
    except (
        QualityError,
        OSError,
        KeyError,
        ValueError,
        subprocess.CalledProcessError,
    ) as error:
        print(f"quality report: FAIL: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
