#!/usr/bin/env python3
"""Fail on `pub fn` items of engine and desktop crates that no production code references.

A public function that only tests call is dead code the compiler cannot see, because
`pub` hides it from the unused-code lint. The check lists every `pub fn` in the
production code of `engine/` and `apps/desktop` whose name appears in no other production Rust file or
line of the repository (`engine/`, `apps/desktop`, and the Rust that
`contracts/generate.py` writes). Test code (`tests/`, `#[cfg(test)]` items, test-support
files) neither defines nor references a name. The match is by name, so a name shared
with another item or a trait method counts as used: the check misses some dead code
and never reports live code, apart from the reviewed entries of
`scripts/dead_pub_allowlist.json`.

Delete the function (and the tests that only exercise it), or, when a table or generated
code reaches it in a way the name does not show, add `"path::name": "reason"` to the
allowlist. An allowlist entry that no longer matches a candidate fails the check, so the
list cannot go stale.
"""

from __future__ import annotations

import re
from collections import Counter
from pathlib import Path

from dead_declarations import problems_of, run
from check_swallowed_errors import (
    blank_literals_and_comments,
    production_files,
    without_test_items,
)

ALLOWLIST = Path(__file__).resolve().with_name("dead_pub_allowlist.json")
UNLISTED = (
    "{path}:{line}: pub fn {name} is never called by production code; delete it "
    "(and the tests that only use it), or allowlist it with a reason"
)
DEFINITION_ROOTS = ("engine", "apps/desktop")
REFERENCE_ROOTS = DEFINITION_ROOTS
PUB_FN = re.compile(r"^[ \t]*pub(?:[ \t]+(?:const|async|unsafe))*[ \t]+fn[ \t]+(\w+)", re.M)
IDENTIFIER = re.compile(r"[A-Za-z_]\w*")
# `#[serde(with = "module")]` calls these two by name.
CALLED_BY_SERDE = {"serialize", "deserialize"}


def production_code(path: Path) -> str:
    code = blank_literals_and_comments(path.read_text(encoding="utf-8"))
    return without_test_items(code)


def referenced_names(root: Path) -> Counter[str]:
    """Count each identifier that is not the name right after `fn` (a definition)."""
    counts: Counter[str] = Counter()
    files = [
        path
        for relative in REFERENCE_ROOTS
        for path in production_files(root, relative, ".rs")
    ]
    # `generated.rs` is excluded from production_files, but generated code calls into
    # the crates, so it counts as a reference.
    files += sorted(path for relative in DEFINITION_ROOTS for path in (root / relative).rglob("generated.rs"))
    for path in files:
        previous = ""
        for name in IDENTIFIER.findall(production_code(path)):
            if previous != "fn":
                counts[name] += 1
            previous = name
    return counts


def candidates(root: Path) -> list[tuple[str, int, str]]:
    """Return (path, line, name) for every unreferenced `pub fn`."""
    counts = referenced_names(root)
    found: list[tuple[str, int, str]] = []
    for path in (
        path
        for relative in DEFINITION_ROOTS
        for path in production_files(root, relative, ".rs")
    ):
        code = production_code(path)
        relative = path.relative_to(root).as_posix()
        for match in PUB_FN.finditer(code):
            name = match.group(1)
            if counts[name] == 0 and name not in CALLED_BY_SERDE:
                found.append((relative, code.count("\n", 0, match.start()) + 1, name))
    return found


problems = problems_of(candidates, UNLISTED, "dead pub fn")


if __name__ == "__main__":
    raise SystemExit(run(__doc__, ALLOWLIST, problems))
