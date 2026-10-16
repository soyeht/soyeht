#!/usr/bin/env python3
"""Fail when production code writes to standard error by hand.

Every log line goes through `soyeht_platform::log` (`event`, `failure`, `Line`): one
function owns the JSON shape, the escaping and the rule that a failure logs its kind and
never its message text. A hand-written `eprintln!(...)` bypasses all three.

Allowed: `engine/soyeht-platform/src/log.rs`, which writes the line, and the `main.rs`
of a binary, which prints the error that ends the process. Test code (`tests/` and
`#[cfg(test)]` items) is not checked. The number of other sites is held at zero.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

from check_swallowed_errors import (
    blank_literals_and_comments,
    production_files,
    without_test_items,
)

ROOT = Path(__file__).resolve().parents[1]
RUST_ROOTS = ("engine", "apps/desktop")
LOG_MODULE = "engine/soyeht-platform/src/log.rs"
# `eprintln!` and `eprint!`. A handle on standard error is not a write: the log rotation
# redirects it.
STDERR_WRITE = re.compile(r"\beprint(?:ln)?\s*!")
ALLOWED_PRINTS = 0


def is_allowed(relative: str) -> bool:
    return relative == LOG_MODULE or relative.endswith("/src/main.rs")


def sites(root: Path) -> list[tuple[str, int]]:
    """Return (path, line) for every hand-written standard error write."""
    found: list[tuple[str, int]] = []
    for relative_root in RUST_ROOTS:
        for path in production_files(root, relative_root, ".rs"):
            relative = path.relative_to(root).as_posix()
            if "/src/" not in relative or is_allowed(relative):
                continue
            code = without_test_items(
                blank_literals_and_comments(path.read_text(encoding="utf-8"))
            )
            for match in STDERR_WRITE.finditer(code):
                found.append((relative, code.count("\n", 0, match.start()) + 1))
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    root = parser.parse_args().root.resolve()
    found = sites(root)
    for path, line in found:
        print(f"logging: {path}:{line}: write to standard error by hand")
    if len(found) > ALLOWED_PRINTS:
        print(
            f"logging: FAIL: {len(found)} hand-written writes, {ALLOWED_PRINTS} allowed; "
            "use soyeht_platform::log::{event, failure, failure_with, Line}",
        )
        return 1
    print(f"logging: PASS: {len(found)} hand-written writes to standard error")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
