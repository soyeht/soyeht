#!/usr/bin/env python3
"""Fail unless measured code quality matches the versioned baseline exactly."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

from check_pane_interior import scan as pane_interior_problems
from gate_base import BASE_BRANCH, resolve_base
from hardening_gate import mask, test_module_files, without_cfg_test
from quality_core import (
    BASELINE_NAME,
    JUSTIFICATIONS_NAME,
    PANIC_CEILING,
    PANIC_KEY,
    Change,
    QualityError,
    baseline_at,
    baseline_from_measurement,
    compare,
    differences,
    justification_for,
    load_justifications,
    measure,
    periphery_status,
    source_files,
    tool_paths,
    unjustified_relaxations,
)


ROOT = Path(__file__).resolve().parents[1]
REFRESH_COMMAND = (
    f"uv run python scripts/check_quality.py --print-baseline > {BASELINE_NAME}"
)
# The objects whose state and behaviour have spread across files. Their files and their open
# fields may only go down: new state belongs in a domain struct with private fields.
CENTRAL_OBJECT_KEY = "central_objects"
CORE_SOURCE = "engine/soyeht-engine/src"
CORE_STRUCT_FILE = f"{CORE_SOURCE}/core.rs"
ENGINE_CLIENT_SOURCE = "apps/shared/Sources"
WORKBENCH_SOURCE = "apps/desktop/crates/soyeht-desktop/src"
INHERENT_IMPL = (
    r"^\s*impl(?:\s*<[^<>]*(?:<[^<>]*(?:<[^<>]*>[^<>]*)*>[^<>]*)*>)?"
    r"\s+(?:[\w]+::)*"
)
CORE_IMPL = re.compile(INHERENT_IMPL + r"Core\b")
WORKBENCH_IMPL = re.compile(INHERENT_IMPL + r"Workbench\b")
ENGINE_CLIENT_EXTENSION = re.compile(r"\bextension\s+EngineClient\b")
CORE_STRUCT = re.compile(r"^\s*pub struct Core\b")
OPEN_FIELD = re.compile(r"^\s*pub(?:\([^)]*\))?\s+[A-Za-z_]\w*\s*:")
TEST_DIRECTORIES = {"tests", "Tests"}


def production_files(repo: Path, relative_dir: str, suffix: str) -> list[Path]:
    """The measured source files of an area, without its test targets and test modules."""
    return [
        path
        for path in source_files(repo, relative_dir, suffix)
        if path.name != "tests.rs"
        and not TEST_DIRECTORIES.intersection(path.relative_to(repo).parts)
    ]


def files_matching(paths: list[Path], pattern: re.Pattern[str]) -> int:
    """How many of `paths` hold at least one line matching `pattern`."""
    return sum(
        1
        for path in paths
        if any(pattern.search(line) for line in path.read_text(encoding="utf-8").splitlines())
    )


def rust_files_matching(
    repo: Path, relative_dir: str, pattern: re.Pattern[str]
) -> int:
    """The production files of a Rust area with a matching line.

    Test modules kept in their own files are found the way
    `quality_core.engine_production_panics` finds them, and comments, strings and
    inline `#[cfg(test)]` items are blanked before the search.
    """
    masked = {
        path.relative_to(repo).as_posix(): mask(path.read_text(encoding="utf-8"))[0]
        for path in production_files(repo, relative_dir, ".rs")
    }
    excluded = test_module_files(masked)
    return sum(
        1
        for path, clean in masked.items()
        if path not in excluded
        and any(
            pattern.search(line) for line in without_cfg_test(clean).splitlines()
        )
    )


def open_core_fields(path: Path) -> int:
    """The `pub`, `pub(crate)` and `pub(super)` fields of the `Core` struct body."""
    if not path.is_file():
        return 0
    lines = path.read_text(encoding="utf-8").splitlines()
    start = next(
        (index for index, line in enumerate(lines) if CORE_STRUCT.search(line)), None
    )
    if start is None:
        return 0
    body = next(
        (index for index in range(start, len(lines)) if "{" in lines[index]), None
    )
    semicolon = next(
        (index for index in range(start, len(lines)) if ";" in lines[index]), len(lines)
    )
    if body is None or body > semicolon:
        return 0  # a unit or tuple struct: there is no named field to count
    count = 0
    depth = 0
    for index in range(body, len(lines)):
        code = lines[index].split("//", 1)[0]
        if depth == 1 and OPEN_FIELD.search(code):
            count += 1
        depth += code.count("{") - code.count("}")
        if index > body and depth <= 0:
            break
    return count


def measure_with_central_objects(repo: Path, jscpd: Path, machete: str) -> dict:
    """The quality measurement of the tree, with the central object counts beside it."""
    current = measure(repo, jscpd, machete)
    current[CENTRAL_OBJECT_KEY] = central_object_counts(repo)
    return current


def central_object_counts(repo: Path) -> dict[str, int]:
    """How far each central object's state and behaviour are spread."""
    return {
        "core_impl_files": rust_files_matching(repo, CORE_SOURCE, CORE_IMPL),
        "core_open_fields": open_core_fields(repo / CORE_STRUCT_FILE),
        "engine_client_extension_files": files_matching(
            production_files(repo, ENGINE_CLIENT_SOURCE, ".swift"),
            ENGINE_CLIENT_EXTENSION,
        ),
        "workbench_impl_files": rust_files_matching(
            repo, WORKBENCH_SOURCE, WORKBENCH_IMPL
        ),
    }


def central_object_changes(baseline: dict, actual: dict) -> list[Change]:
    """Every central-object count that differs from the checked-in baseline."""
    previous_counts = baseline.get(CENTRAL_OBJECT_KEY, {})
    current_counts = actual.get(CENTRAL_OBJECT_KEY, {})
    changes: list[Change] = []
    for name in sorted(previous_counts.keys() | current_counts.keys()):
        previous = previous_counts.get(name, 0)
        current = current_counts.get(name, 0)
        changes += compare(
            f"{CENTRAL_OBJECT_KEY}.{name}",
            previous,
            current,
            f"central object {name}: baseline {previous}, actual {current}",
            "; move new state into a domain struct with private fields",
        )
    return changes


def unjustified_central_objects(
    previous: dict | None, baseline: dict, justifications: dict[str, dict]
) -> list[Change]:
    """Raised central-object counts since `previous` without a written reason."""
    if previous is None or CENTRAL_OBJECT_KEY not in previous:
        return []
    return [
        change
        for change in central_object_changes(previous, baseline)
        if change.direction == "RELAXATION"
        and not justification_for(change, justifications)
    ]


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
        current = measure_with_central_objects(root, jscpd, machete)
        if args.print_baseline:
            baseline = baseline_from_measurement(current)
            baseline[CENTRAL_OBJECT_KEY] = current[CENTRAL_OBJECT_KEY]
            print(json.dumps(baseline, indent=2, sort_keys=True))
            return 0
        expected = json.loads((root / BASELINE_NAME).read_text())
        if expected["version"] != current["version"]:
            raise QualityError("quality baseline version differs from the scanner")
        changes = differences(expected, current) + central_object_changes(expected, current)
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
        justifications = load_justifications(root)
        previous = baseline_at(root, base)
        unjustified = unjustified_relaxations(previous, expected, justifications)
        unjustified += unjustified_central_objects(previous, expected, justifications)
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
