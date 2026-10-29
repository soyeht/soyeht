#!/usr/bin/env python3
"""Count error-swallowing forms in production code for the quality ratchet.

Swift `try?` and Rust `.ok()` / `let _ =` discard an error (`let _ = expr?;` propagates
it and is not counted). A Rust `Err(_) =>` arm drops the error it matches the same way,
for example `Err(_) => None` or `Err(_) => break`; give the error a name and log it, or
keep the arm with a one-line comment saying why the error carries nothing worth keeping.
The totals are the `swallowed_errors` entries of scripts/quality_baseline.json, which
scripts/check_quality.py holds exactly. Test code (Swift `Tests` directories, Rust
`tests/` directories, `tests.rs` files and `#[cfg(test)]` items) is not counted.

Two forms are not counted, because neither loses a failure the logs should know about:

- A Rust `.ok()` directly on a pure conversion (`parse`, `try_from`, `from_utf8`,
  `parse_from_rfc3339`, ... the names of `CONVERSIONS`). The call does no IO and the only
  failure it has is "this text is not that type", which `None` already says.
- A site with a `// ignored: <reason>` comment: at the end of the site's line, or alone on the
  line(s) directly above it (a reason may continue on following `//` lines). The reason says
  why dropping the error is correct. A marker with no reason does not count.

Run this script to print every counted site, and `--exempt` to print the others with the
form that exempts them.
"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SWIFT_AREAS = {
    "shared": "apps/shared/Sources",
    "mac": "apps/mac/Sources",
    "ios": "apps/ios/Sources",
}
RUST_ROOTS = ("engine", "apps/desktop")
IGNORED_PARTS = {"target", ".build", ".cache", "node_modules", "tests", "Tests"}
# Test-only helper modules compiled under `#[cfg(test)]` or a test-support feature.
IGNORED_FILES = {
    "generated.rs",
    "Generated.swift",
    "tests.rs",
    "test_support.rs",
    "test_fixture.rs",
    "testing.rs",
}
PATTERNS = {
    "swift_try_optional": re.compile(r"\btry\?"),
    "rust_ok": re.compile(r"\.ok\(\)"),
    "rust_let_underscore": re.compile(r"\blet\s+_\s*="),
    # `Err(_) =>`, also with a guard up to its arrow: `Err(_) if left == right =>`.
    "rust_err_wildcard": re.compile(r"\bErr\(\s*_\s*\)\s*(?:if\b(?:(?!=>)[\s\S])*)?=>"),
}
CFG_TEST = re.compile(r"#\[cfg\(\s*(?:test|all\(\s*test\b[^\]]*)\)\]")
INNER_CFG_TEST = re.compile(r"\A\s*#!\[cfg\(test\)\]")
OTHER_ATTRIBUTE = re.compile(r"\s*#\[")
ITEM_OR_STATEMENT = re.compile(
    r"\s*(?:pub\b|mod\b|fn\b|impl\b|use\b|const\b|static\b|struct\b|enum\b|trait\b"
    r"|type\b|async\b|unsafe\b|extern\b|let\b|if\b|match\b|for\b|while\b|loop\b|\{)"
)
# `// ignored: <reason>`: the marker needs a reason after the colon.
IGNORED_COMMENT = re.compile(r"//[/!]?\s*ignored:\s*\S")
# Calls that only turn text or a number into another type. They do no IO, and the one way they
# fail is "this is not that type", which the `None` of `.ok()` already says.
CONVERSIONS = frozenset(
    {
        "decode",
        "duration_since",
        "from_bytes_until_nul",
        "from_slice",
        "from_str",
        "from_str_radix",
        "from_utf16",
        "from_utf8",
        "from_value",
        "parse",
        "parse_from_rfc2822",
        "parse_from_rfc3339",
        "parse_from_str",
        "parse_str",
        "strip_prefix",
        "to_file_path",
        "to_value",
        "to_vec",
        "try_from",
        "try_into",
    }
)
RAW_STRING = re.compile(r'b?r(#*)"')


def blank_literals_and_comments(source: str) -> str:
    """Replace comments and string/char literal contents with blanks, keeping lines."""
    return lex(source)[0]


def lex(source: str) -> tuple[str, list[tuple[int, str]]]:
    """The source with comments and literal contents blanked, and its `ignored:` comments.

    An `ignored:` comment is (line number, text from its `//`); no other comment is kept.
    """
    out: list[str] = []
    comments: list[tuple[int, str]] = []
    i, n = 0, len(source)
    while i < n:
        c = source[i]
        nxt = source[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":
            end = source.find("\n", i)
            end = n if end < 0 else end
            if "ignored:" in source[i:end]:
                comments.append((source.count("\n", 0, i) + 1, source[i:end]))
            out.append(" " * (end - i))
            i = end
        elif c == "/" and nxt == "*":
            end = source.find("*/", i + 2)
            end = n if end < 0 else end + 2
            out.append(re.sub(r"[^\n]", " ", source[i:end]))
            i = end
        elif (raw := RAW_STRING.match(source, i)) and not (
            i and (source[i - 1].isalnum() or source[i - 1] == "_")
        ):
            close = '"' + raw.group(1)
            end = source.find(close, raw.end())
            end = n if end < 0 else end + len(close)
            out.append(re.sub(r"[^\n]", " ", source[i:end]))
            i = end
        elif c == '"':
            j = i + 1
            while j < n and source[j] != '"':
                j += 2 if source[j] == "\\" else 1
            end = min(j + 1, n)
            out.append('"' + re.sub(r"[^\n]", " ", source[i + 1 : end - 1]) + '"')
            i = end
        elif c == "'" and re.match(r"'(?:\\.|[^\\'])'", source[i : i + 4]):
            literal = re.match(r"'(?:\\.|[^\\'])'", source[i : i + 4]).group(0)
            out.append("' '" + " " * (len(literal) - 3))
            i += len(literal)
        else:
            out.append(c)
            i += 1
    return "".join(out), comments


def without_test_items(code: str) -> str:
    """Blank every item, statement or field that follows a `#[cfg(test)]` attribute.

    A file that starts with `#![cfg(test)]` is blanked whole.
    """
    if INNER_CFG_TEST.search(code):
        return re.sub(r"[^\n]", " ", code)
    chars = list(code)
    copied = 0
    for match in CFG_TEST.finditer(code):
        if match.start() < copied:
            continue
        end = test_item_end(code, match.end())
        for k in range(match.start(), end):
            if chars[k] != "\n":
                chars[k] = " "
        copied = end
    return "".join(chars)


def test_item_end(code: str, start: int) -> int:
    """The offset after the item, statement or field that begins after an attribute."""
    index = start
    while True:
        match = OTHER_ATTRIBUTE.match(code, index)
        if not match:
            break
        index = skip_balanced(code, match.end() - 1)
    # An item or a statement ends at its `;` or its closing brace; a field or an
    # argument, which starts with a name, ends at the next comma.
    ends_at_comma = not ITEM_OR_STATEMENT.match(code, index)
    depth = 0
    while index < len(code):
        char = code[index]
        if char in "([{":
            depth += 1
        elif char in ")]}":
            if depth == 0:
                return index
            depth -= 1
            if depth == 0 and char == "}" and not ends_at_comma:
                return index + 1
        elif depth == 0 and (char == ";" or (ends_at_comma and char == ",")):
            return index + 1
        index += 1
    return len(code)


def skip_balanced(code: str, opening: int) -> int:
    """The offset after the bracket group that opens at `opening`."""
    depth = 0
    for index in range(opening, len(code)):
        if code[index] in "([{":
            depth += 1
        elif code[index] in ")]}":
            depth -= 1
            if depth == 0:
                return index + 1
    return len(code)


def production_files(root: Path, relative: str, suffix: str) -> list[Path]:
    return sorted(
        path
        for path in (root / relative).rglob(f"*{suffix}")
        if path.is_file()
        and not IGNORED_PARTS.intersection(path.relative_to(root).parts)
        and path.name not in IGNORED_FILES
    )


def sites(root: Path) -> list[tuple[str, str, int]]:
    """Return (metric, path, line) for every counted site."""
    return [(metric, path, line) for metric, path, line, exempt in classified(root) if not exempt]


def classified(root: Path) -> list[tuple[str, str, int, str]]:
    """Every site as (metric, path, line, exemption); the exemption is empty when counted."""
    found: list[tuple[str, str, int, str]] = []
    files = [
        (path, ("swift_try_optional",))
        for relative in SWIFT_AREAS.values()
        for path in production_files(root, relative, ".swift")
    ] + [
        (path, ("rust_ok", "rust_let_underscore", "rust_err_wildcard"))
        for relative in RUST_ROOTS
        for path in production_files(root, relative, ".rs")
    ]
    for path, metrics in files:
        source = path.read_text(encoding="utf-8")
        code, comments = lex(source)
        source_lines = source.split("\n")
        if path.suffix == ".rs":
            code = without_test_items(code)
        lines = code.split("\n")
        marked = ignored_lines(comments, lines, source_lines)
        relative = path.relative_to(root).as_posix()
        for metric in metrics:
            for match in PATTERNS[metric].finditer(code):
                if metric == "rust_let_underscore" and propagates(code, match.end()):
                    continue
                line = code.count("\n", 0, match.start()) + 1
                found.append((metric, relative, line, exemption(metric, code, match, line, marked)))
    return found


def exemption(metric: str, code: str, match: re.Match, line: int, marked: set[int]) -> str:
    """Why a site is not counted: "conversion", "ignored", or "" when it is counted."""
    if metric == "rust_ok" and converts(code, match.start()):
        return "conversion"
    if line in marked:
        return "ignored"
    return ""


def ignored_lines(
    comments: list[tuple[int, str]], lines: list[str], source_lines: list[str]
) -> set[int]:
    """The lines a `// ignored: <reason>` comment speaks for.

    A comment at the end of a line of code speaks for that line. A comment alone on its line
    speaks for the first line after it and the comment lines that continue it.
    """
    covered: set[int] = set()
    for number, text in comments:
        if not IGNORED_COMMENT.match(text):
            continue
        if lines[number - 1].strip():
            covered.add(number)
            continue
        following = number + 1
        while following <= len(source_lines) and source_lines[following - 1].lstrip().startswith(
            "//"
        ):
            following += 1
        covered.add(following)
    return covered


def converts(code: str, dot: int) -> bool:
    """True when the expression before the `.ok()` at `dot` ends in a pure conversion call."""
    index = dot
    while index > 0 and code[index - 1].isspace():
        index -= 1
    if index == 0 or code[index - 1] != ")":
        return False
    index = matching_open(code, index - 1, ")", "(")
    while index > 0 and code[index - 1].isspace():
        index -= 1
    if index > 0 and code[index - 1] == ">":
        # A turbofish: `parse::<u32>()`.
        index = matching_open(code, index - 1, ">", "<")
        while index > 0 and code[index - 1].isspace():
            index -= 1
        if code[max(index - 2, 0) : index] == "::":
            index -= 2
    name = re.search(r"[A-Za-z_][A-Za-z0-9_]*\Z", code[max(index - 64, 0) : index])
    return bool(name) and name.group(0) in CONVERSIONS


def matching_open(code: str, close: int, closing: str, opening: str) -> int:
    """The offset of the bracket that opens the group closed at `close`, or 0."""
    depth = 0
    for index in range(close, -1, -1):
        if code[index] == closing:
            depth += 1
        elif code[index] == opening:
            depth -= 1
            if depth == 0:
                return index
    return 0


def propagates(code: str, start: int) -> bool:
    """True for `let _ = expr?;`: the error propagates and only the value is dropped."""
    depth = 0
    for index in range(start, len(code)):
        char = code[index]
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
        elif char == ";" and depth == 0:
            return code[start:index].rstrip().endswith("?")
    return False


def measure(root: Path) -> dict[str, int]:
    counts = dict.fromkeys(PATTERNS, 0)
    for metric, _, _ in sites(root):
        counts[metric] += 1
    return counts


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    parser.add_argument(
        "--exempt", action="store_true", help="also print the sites that are not counted"
    )
    arguments = parser.parse_args()
    root = arguments.root.resolve()
    for metric, path, line, exempt in classified(root):
        if exempt and not arguments.exempt:
            continue
        print(f"{path}:{line}: {metric}" + (f" (exempt: {exempt})" if exempt else ""))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
