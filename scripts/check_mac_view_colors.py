#!/usr/bin/env python3
"""Reject direct color construction in Mac and iPhone views."""

from __future__ import annotations

import pathlib
import re
import sys


COLOR_CALL = re.compile(r"\bColor\s*\(")
SYSTEM_COLOR = (
    r"(?:accentColor|black|blue|brown|cyan|gray|green|indigo|mint|orange|pink|"
    r"purple|red|teal|white|yellow|primary|secondary|tertiary|quaternary)"
)
QUALIFIED_COLOR = re.compile(rf"\bColor\s*\.\s*{SYSTEM_COLOR}\b")
DIRECT_STYLE = re.compile(
    rf"\.(?:foregroundStyle|foregroundColor|background|fill|stroke|tint|border)"
    rf"\s*\(\s*\.{SYSTEM_COLOR}\b"
)
DIRECT_SHADOW = re.compile(
    rf"\.shadow\s*\(\s*(?:\.inner\s*\(\s*)?color\s*:\s*\.{SYSTEM_COLOR}\b"
)
SOURCE_ROOTS = (pathlib.Path("apps/mac/Sources"), pathlib.Path("apps/ios/Sources"))
THEME_COLORS = pathlib.Path("apps/shared/Sources/SoyehtClient/ThemeColors.swift")
HEX_LITERAL = re.compile(r'#[0-9A-Fa-f]{6,8}')


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
    offsets += [match.start() for match in QUALIFIED_COLOR.finditer(code)]
    offsets += [match.start() for match in DIRECT_STYLE.finditer(code)]
    offsets += [match.start() for match in DIRECT_SHADOW.finditer(code)]
    return sorted({source.count("\n", 0, offset) + 1 for offset in offsets})


def swift_source_paths(root: pathlib.Path = pathlib.Path(".")) -> list[pathlib.Path]:
    return sorted(path for source in SOURCE_ROOTS for path in (root / source).rglob("*.swift"))


def scan(root: pathlib.Path = pathlib.Path(".")) -> list[tuple[pathlib.Path, int]]:
    return [
        (path, line)
        for path in swift_source_paths(root)
        for line in violations(path.read_text(encoding="utf-8"))
    ]


def theme_catalog_violations(root: pathlib.Path = pathlib.Path(".")) -> list[str]:
    """The theme catalogue, its names and its palettes are generated from
    contracts/design-tokens.json (`generate.py --check`); this only guards how ThemeColors
    projects them."""
    colors_path = root / THEME_COLORS
    if not colors_path.is_file():
        return [f"missing required theme source: {colors_path.relative_to(root)}"]
    raw_colors = colors_path.read_text(encoding="utf-8")
    colors = code_only(raw_colors)
    problems: list[str] = []
    if HEX_LITERAL.search(raw_colors):
        problems.append("ThemeColors must not contain literal hex values")
    if "SoyehtThemeColorSet(palette: palette)" not in colors:
        problems.append("ThemeColors must project every color from the selected palette")
    palette_sources = (
        "TerminalColorTheme.seededDefault.palette",
        "ThemeStore.shared.activeTheme.palette",
    )
    if not any(source in colors for source in palette_sources):
        problems.append("ThemeColors palette must come from the closed theme catalogue")
    return problems


def main() -> int:
    found = scan()
    for path, line in found:
        print(f"{path}:{line}: view colors must come from the platform theme")
    catalogue = theme_catalog_violations()
    for problem in catalogue:
        print(f"theme catalogue: {problem}")
    return 1 if found or catalogue else 0


if __name__ == "__main__":
    sys.exit(main())
