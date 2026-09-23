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
    DUPLICATION_SCANS,
    QualityError,
    baseline_relaxations,
    differences,
    measure,
    periphery_status,
    tool_paths,
)


ROOT = Path(__file__).resolve().parents[1]
BASELINE_NAME = "scripts/quality_baseline.json"


def base_measurement(commit: str, jscpd: Path, machete: str) -> dict:
    existing = subprocess.run(
        ["git", "show", f"{commit}:{BASELINE_NAME}"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if existing.returncode == 0:
        return json.loads(existing.stdout)
    # The first Q1 PR has no baseline at its base. Measure its archived tree.
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


def render(before: dict, after: dict, current: dict, base: str, periphery: str) -> str:
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
        for name, value in current["loose_literals"][area].items():
            old = before["loose_literals"][area][name]
            lines.append(f"literal {area}.{name}: {old} -> {value}")
    lines.append("cargo-machete: PASS")
    lines.append(f"Periphery: {periphery}")
    relaxations = baseline_relaxations(before, after)
    if relaxations:
        lines.append(
            "🔴 BASELINE RELAXATION — written justification required in the PR:"
        )
        lines.extend(f"🔴 {explanation}" for explanation in relaxations)
    else:
        lines.append("Baseline relaxations: none")
    drift = differences(after, current)
    if drift:
        lines.append("FAIL: current measurements and checked-in baseline differ:")
        lines.extend(
            f"  {direction}: {explanation}" for direction, explanation in drift
        )
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
        after = json.loads((ROOT / BASELINE_NAME).read_text())
        current = measure(ROOT, jscpd, machete)
        print(render(before, after, current, commit[:12], periphery_status(ROOT)))
        return 1 if differences(after, current) else 0
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
