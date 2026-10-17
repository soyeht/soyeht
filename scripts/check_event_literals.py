#!/usr/bin/env python3
"""Hold down the events and results the engine still writes as untyped JSON.

The engine builds an event with the generated type of its schema (`PaneTurnEndedDraft` and
the other `...Draft` types, `model::pending`), which a renamed or misspelt field cannot
pass. A `json!({... "paneId": ...})` literal names its fields by hand, and every such site is a
place where the engine and the contract can drift apart without a compile error. The number
of literals with a `"paneId"` key at any depth, in the production code of `soyeht-engine`, may
only go down.

The allowed count is `pane_id_literals` in scripts/event_literals_baseline.json. When the
count falls, lower the baseline in the same change:

    uv run python scripts/check_event_literals.py --print-baseline > scripts/event_literals_baseline.json

The engine's `contract-checks` feature checks the remaining literals against the contract at
run time (ADR 0090).
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from check_swallowed_errors import (
    blank_literals_and_comments,
    production_files,
    skip_balanced,
    without_test_items,
)

ROOT = Path(__file__).resolve().parents[1]
ENGINE_SOURCE = "engine/soyeht-engine/src"
BASELINE = Path(__file__).resolve().parent / "event_literals_baseline.json"
KEY = "pane_id_literals"
# `#[cfg(test)] mod name;`: the module is a file that only test builds compile.
TEST_MODULE = re.compile(r"#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;")
JSON_LITERAL = re.compile(r"\bjson!\s*\(")
# The pane id as a key, at any depth of the literal.
PANE_ID_KEY = re.compile(r'"paneId"\s*:')


def sites(root: Path) -> list[tuple[str, int]]:
    """Return (path, line) for every `json!(...)` literal with a `"paneId"` key in production code."""
    found: list[tuple[str, int]] = []
    files = production_files(root, ENGINE_SOURCE, ".rs")
    sources = {path: path.read_text(encoding="utf-8") for path in files}
    test_modules = {name for source in sources.values() for name in TEST_MODULE.findall(source)}
    for path, source in sources.items():
        if test_modules.intersection({path.stem, path.parent.name}):
            continue
        # Offsets agree: the test items are blanked in place, so a match inside one is skipped.
        production = without_test_items(blank_literals_and_comments(source))
        for match in JSON_LITERAL.finditer(source):
            if not production[match.start()].strip():
                continue
            # The blanked text balances the brackets; the key is read from the source.
            end = skip_balanced(production, match.end() - 1)
            if PANE_ID_KEY.search(source, match.end(), end):
                found.append(
                    (path.relative_to(root).as_posix(), source.count("\n", 0, match.start()) + 1)
                )
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    parser.add_argument("--baseline", type=Path, default=BASELINE, help=argparse.SUPPRESS)
    parser.add_argument("--print-baseline", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    found = sites(root)
    if args.print_baseline:
        print(json.dumps({KEY: len(found)}, indent=2))
        return 0
    allowed = json.loads(args.baseline.read_text(encoding="utf-8"))[KEY]
    if len(found) > allowed:
        for path, line in found:
            print(f"event literals: {path}:{line}: json!(... \"paneId\" ...)")
        print(
            f"event literals: FAIL: {len(found)} untyped literals, {allowed} allowed; build the "
            "event from its generated type (model::pending) instead"
        )
        return 1
    if len(found) < allowed:
        print(
            f"event literals: FAIL: {len(found)} untyped literals, baseline {allowed}; lower it "
            "with --print-baseline"
        )
        return 1
    print(f"event literals: PASS: {len(found)} untyped paneId literals")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
