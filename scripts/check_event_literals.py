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

import re
from pathlib import Path

from check_swallowed_errors import (
    blank_literals_and_comments,
    production_files,
    skip_balanced,
    without_test_items,
)
from count_ratchet import run

ENGINE_SOURCE = "engine/soyeht-engine/src"
BASELINE = Path(__file__).resolve().parent / "event_literals_baseline.json"
KEY = "pane_id_literals"
# `#[cfg(test)] mod name;`: the module is a file that only test builds compile.
TEST_MODULE = re.compile(r"#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;")
JSON_LITERAL = re.compile(r"\bjson!\s*\(")
# The pane id as a key, at any depth of the literal.
PANE_ID_KEY = re.compile(r'"paneId"\s*:')


def production_sources(root: Path):
    """Yield (path, source, production) for each production file of the engine.

    `production` is the source with literals, comments and test items blanked in place, so an
    offset in it is an offset in `source`; a file only test builds compile is left out.
    """
    files = production_files(root, ENGINE_SOURCE, ".rs")
    sources = {path: path.read_text(encoding="utf-8") for path in files}
    test_modules = {name for source in sources.values() for name in TEST_MODULE.findall(source)}
    for path, source in sources.items():
        if test_modules.intersection({path.stem, path.parent.name}):
            continue
        yield path, source, without_test_items(blank_literals_and_comments(source))


def sites(root: Path) -> list[tuple[str, int]]:
    """Return (path, line) for every `json!(...)` literal with a `"paneId"` key in production code."""
    found: list[tuple[str, int]] = []
    for path, source, production in production_sources(root):
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
    return run(
        description=__doc__,
        label="event literals",
        key=KEY,
        baseline=BASELINE,
        find=sites,
        site='json!(... "paneId" ...)',
        noun="untyped paneId literals",
        fix="build the event from its generated type (model::pending) instead",
        baseline_name=BASELINE.name,
    )


if __name__ == "__main__":
    raise SystemExit(main())
