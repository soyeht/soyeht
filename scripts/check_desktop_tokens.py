#!/usr/bin/env python3
"""Fail on derived design sizes that the desktop views write by hand.

A size of the desktop is a token of `contracts/design-tokens.json` (`theme::desktop`,
`type_scale`, `glyph`, ...). `px(type_scale::BODY - 1.0)` is a size no token names: it drifts
from the design file and from the Mac, and nothing says why it is one less. The check lists,
in the production code of the desktop client outside `theme/`:

- a `px(...)` or a `const NAME: f32 = ...` that adds a number to, or takes one from, a token of
  `type_scale`, `glyph`, `desktop`, `pane_interior`, `layout` or `composer`;
- a number as the radius of `neo::raised(...)` or `neo::well(...)`.

Name the size in `design-tokens.json`, run `contracts/generate.py` and use the new token.
Multiplying or dividing by a number (to center, or to pad both sides) is allowed, and so is
arithmetic on `space::X*` units: they are a ladder, not sizes.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

from check_swallowed_errors import (
    blank_literals_and_comments,
    production_files,
    skip_balanced,
    without_test_items,
)

ROOT = Path(__file__).resolve().parents[1]
DESKTOP_SRC = "apps/desktop/crates/soyeht-desktop/src"
TOKEN = re.compile(
    r"\b(?:type_scale|glyph|desktop|pane_interior|layout|composer)::[A-Z][A-Z0-9_]*"
)
# A number added to or taken from a token. A number that multiplies or divides (`/ 2.0` to
# center, `2.0 * PADDING` for both sides) counts things and is allowed.
ARITHMETIC_WITH_NUMBER = re.compile(
    r"[-+]\s*\d+(?:\.\d+)?(?![\d.])(?!\s*[*/])|\b\d+(?:\.\d+)?\s*[-+]\s*[A-Za-z_(]"
)
SIZE_EXPRESSION = re.compile(r"\bpx\s*\(|\bconst\s+[A-Z][A-Z0-9_]*\s*:\s*f32\s*=")
NEO_RADIUS = re.compile(r"\bneo::(?:raised|well)\s*\(\s*[^,()]+,\s*\d")


def expression_after(code: str, start: int) -> str:
    """The text of a `px(...)` call, or of a const's value up to its `;`."""
    if code[start - 1] == "(":
        return code[start : skip_balanced(code, start - 1)]
    end = code.find(";", start)
    return code[start : end if end >= 0 else len(code)]


def problems(root: Path) -> list[str]:
    found: list[str] = []
    for path in production_files(root, DESKTOP_SRC, ".rs"):
        relative = path.relative_to(root).as_posix()
        if "/theme/" in relative:
            continue
        code = without_test_items(blank_literals_and_comments(path.read_text(encoding="utf-8")))
        for match in SIZE_EXPRESSION.finditer(code):
            expression = expression_after(code, match.end())
            if TOKEN.search(expression) and ARITHMETIC_WITH_NUMBER.search(expression):
                line = code.count("\n", 0, match.start()) + 1
                found.append(
                    f"{relative}:{line}: a token with a number added or taken away; "
                    "name the size in contracts/design-tokens.json"
                )
        for match in NEO_RADIUS.finditer(code):
            line = code.count("\n", 0, match.start()) + 1
            found.append(
                f"{relative}:{line}: a number as the radius of a neo surface; "
                "use a token of theme::radius or theme::desktop"
            )
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    messages = problems(parser.parse_args().root.resolve())
    for message in messages:
        print(message)
    return 1 if messages else 0


if __name__ == "__main__":
    raise SystemExit(main())
