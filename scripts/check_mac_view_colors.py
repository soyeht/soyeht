#!/usr/bin/env python3
"""Reject direct color construction in Mac views."""

from __future__ import annotations

import pathlib
import re
import sys


COLOR_CALL = re.compile(r"\bColor\s*\(")
DIRECT_STYLE = re.compile(
    r"\.(?:foregroundStyle|background|fill|stroke|tint|border)\s*\(\s*"
    r"\.(?:black|blue|brown|cyan|gray|green|indigo|mint|orange|pink|purple|red|"
    r"teal|white|yellow|primary|secondary)\b"
)


def code_only(source: str) -> str:
    result = list(source)
    index = 0
    block_depth = 0
    while index < len(source):
        pair = source[index : index + 2]
        if block_depth:
            if pair == "/*":
                block_depth += 1
                result[index : index + 2] = "  "
                index += 2
            elif pair == "*/":
                block_depth -= 1
                result[index : index + 2] = "  "
                index += 2
            else:
                if source[index] != "\n":
                    result[index] = " "
                index += 1
            continue
        if pair == "//":
            newline = source.find("\n", index + 2)
            end = len(source) if newline < 0 else newline
            result[index:end] = " " * (end - index)
            index = end
            continue
        if pair == "/*":
            block_depth = 1
            result[index : index + 2] = "  "
            index += 2
            continue
        if source[index] == '"':
            multiline = source.startswith('"""', index)
            marker = '"""' if multiline else '"'
            end = index + len(marker)
            while end < len(source):
                if source.startswith(marker, end) and (multiline or source[end - 1] != "\\"):
                    end += len(marker)
                    break
                end += 1
            for position in range(index, min(end, len(source))):
                if source[position] != "\n":
                    result[position] = " "
            index = end
            continue
        index += 1
    return "".join(result)


def violations(source: str) -> list[int]:
    code = code_only(source)
    offsets = [match.start() for match in COLOR_CALL.finditer(code)]
    offsets += [match.start() for match in DIRECT_STYLE.finditer(code)]
    return sorted({source.count("\n", 0, offset) + 1 for offset in offsets})


def main() -> int:
    root = pathlib.Path("apps/mac/Sources")
    found = False
    for path in sorted(root.rglob("*.swift")):
        for line in violations(path.read_text(encoding="utf-8")):
            print(f"{path}:{line}: Mac view colors must come from MacTheme")
            found = True
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
