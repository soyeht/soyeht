#!/usr/bin/env python3
"""Reject SwiftUI Text views that select the system font directly."""

from __future__ import annotations

import pathlib
import sys


def matching_parenthesis(source: str, opening: int) -> int | None:
    depth = 0
    quote: str | None = None
    escaped = False
    index = opening
    while index < len(source):
        character = source[index]
        following = source[index : index + 2]
        if quote:
            if escaped:
                escaped = False
            elif character == "\\":
                escaped = True
            elif character == quote:
                quote = None
        elif following == "//":
            newline = source.find("\n", index + 2)
            index = len(source) if newline < 0 else newline
        elif following == "/*":
            closing = source.find("*/", index + 2)
            index = len(source) if closing < 0 else closing + 1
        elif character in {'"', "'"}:
            quote = character
        elif character == "(":
            depth += 1
        elif character == ")":
            depth -= 1
            if depth == 0:
                return index
        index += 1
    return None


def violations(source: str) -> list[int]:
    lines: list[int] = []
    cursor = 0
    while True:
        start = source.find("Text", cursor)
        if start < 0:
            return lines
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
        tail = closing + 1
        while tail < len(source) and source[tail].isspace():
            tail += 1
        if source.startswith(".font", tail):
            font = tail + len(".font")
            compact = "".join(source[font : font + 80].split())
            if compact.startswith("(.system("):
                lines.append(source.count("\n", 0, start) + 1)
        cursor = closing + 1


def main() -> int:
    root = pathlib.Path("apps/mac/Sources")
    found = False
    for path in sorted(root.rglob("*.swift")):
        for line in violations(path.read_text(encoding="utf-8")):
            print(f"{path}:{line}: SwiftUI Text must use MacTheme.chrome or MacTheme.mono")
            found = True
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
