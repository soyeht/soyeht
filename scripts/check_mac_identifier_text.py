#!/usr/bin/env python3
"""Require explicit identifier interpolation in Mac Text literals."""

from __future__ import annotations

import pathlib
import re
import sys

from check_mac_text_fonts import matching_parenthesis


IDENTIFIER_EXPRESSION = re.compile(
    r"(?:"
    r"\bserver\.(?:port|pid)\b|"
    r"\bscene\.ordinal\b|"
    r"\bpage\s*\(\s*for\s*:|"
    r"\bvariants\.selectedIndex\b|"
    r"\battempt\.n\b|"
    r"\bmatch\.range\.start\.(?:line|column)\b|"
    r"\bsymbol\.selectionRange\.start\.line\b|"
    r"\bslide\.index\b|"
    r"\bindex\s*\+\s*1\b|"
    r"\bhunk\.line\b|"
    r"\bline\s*\?\?|"
    r"\bselectedPosition\?\.(?:line|column)\b|"
    r"\bview\.viewport\?\.fromLine\b|"
    r"\bversion\.revision\b|"
    r"\bviewport\.(?:toLine|width|height)\b"
    r")"
)
IDENTIFIER_LABEL = re.compile(
    r'(?:\b(?:port|pid|line|scene|attempt)\s+|["\'](?:p\.|r))$', re.I
)


def text_calls(source: str):
    cursor = 0
    while True:
        start = source.find("Text", cursor)
        if start < 0:
            return
        before = source[start - 1] if start else " "
        after = source[start + 4] if start + 4 < len(source) else " "
        if before.isalnum() or before == "_" or after.isalnum() or after == "_":
            cursor = start + 4
            continue
        opening = start + 4
        while opening < len(source) and source[opening].isspace():
            opening += 1
        if opening >= len(source) or source[opening] != "(":
            cursor = start + 4
            continue
        closing = matching_parenthesis(source, opening)
        if closing is None:
            cursor = opening + 1
            continue
        yield start, opening, closing
        cursor = closing + 1


def violations(source: str) -> list[int]:
    lines: list[int] = []
    for start, opening, closing in text_calls(source):
        cursor = opening + 1
        while True:
            interpolation = source.find(r"\(", cursor, closing)
            if interpolation < 0:
                break
            expression_opening = interpolation + 1
            expression_closing = matching_parenthesis(source, expression_opening)
            if expression_closing is None or expression_closing > closing:
                break
            expression = source[expression_opening + 1 : expression_closing].strip()
            prefix = source[opening + 1 : interpolation]
            if not expression.startswith("identifier:") and (
                IDENTIFIER_EXPRESSION.search(expression) or IDENTIFIER_LABEL.search(prefix)
            ):
                lines.append(source.count("\n", 0, start) + 1)
                break
            cursor = expression_closing + 1
    return sorted(set(lines))


def main() -> int:
    root = pathlib.Path("apps/mac/Sources")
    found = False
    for path in sorted(root.rglob("*.swift")):
        for line in violations(path.read_text(encoding="utf-8")):
            print(
                f"{path}:{line}: identifiers in Text must use the identifier interpolation"
            )
            found = True
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
