#!/usr/bin/env python3
"""Count error-swallowing forms in production code for the quality ratchet.

Swift `try?` and Rust `.ok()` / `let _ =` discard an error (`let _ = expr?;` propagates
it and is not counted). Each one is either a deliberate, commented choice (best-effort
cleanup, optional probes) or a failure the user or the logs should know about. The
totals are the `swallowed_errors` entries of scripts/quality_baseline.json, which
scripts/check_quality.py holds exactly. Test code (Swift `Tests` directories, Rust
`tests/` directories, `tests.rs` files and `#[cfg(test)]` items) is not counted.

Run this script to print every counted site.
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
RUST_ROOT = "engine"
IGNORED_PARTS = {"target", ".build", ".cache", "node_modules", "tests", "Tests"}
# Test-only helper modules compiled under `#[cfg(test)]` or a test-support feature.
IGNORED_FILES = {
    "generated.rs",
    "Generated.swift",
    "tests.rs",
    "test_support.rs",
    "test_fixture.rs",
}
PATTERNS = {
    "swift_try_optional": re.compile(r"\btry\?"),
    "rust_ok": re.compile(r"\.ok\(\)"),
    "rust_let_underscore": re.compile(r"\blet\s+_\s*="),
}
CFG_TEST = re.compile(r"#\[cfg\(test\)\]")
RAW_STRING = re.compile(r'b?r(#*)"')


def blank_literals_and_comments(source: str) -> str:
    """Replace comments and string/char literal contents with example, keeping lines."""
    out: list[str] = []
    i, n = 0, len(source)
    while i < n:
        c = source[i]
        nxt = source[i + 1] if i + 1 < n else ""
        if c == "/" and nxt == "/":
            end = source.find("\n", i)
            end = n if end < 0 else end
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
    return "".join(out)


def without_test_items(code: str) -> str:
    """Blank every item that follows a `#[cfg(test)]` attribute."""
    chars = list(code)
    for match in reversed(list(CFG_TEST.finditer(code))):
        depth, j = 0, match.end()
        while j < len(code):
            if code[j] == "{":
                depth += 1
            elif code[j] == "}":
                depth -= 1
                if depth == 0:
                    break
            elif code[j] == ";" and depth == 0:
                break
            j += 1
        for k in range(match.start(), min(j + 1, len(code))):
            if chars[k] != "\n":
                chars[k] = " "
    return "".join(chars)


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
    found: list[tuple[str, str, int]] = []
    files = [
        (path, ("swift_try_optional",))
        for relative in SWIFT_AREAS.values()
        for path in production_files(root, relative, ".swift")
    ] + [
        (path, ("rust_ok", "rust_let_underscore"))
        for path in production_files(root, RUST_ROOT, ".rs")
    ]
    for path, metrics in files:
        code = blank_literals_and_comments(path.read_text(encoding="utf-8"))
        if path.suffix == ".rs":
            code = without_test_items(code)
        relative = path.relative_to(root).as_posix()
        for metric in metrics:
            for match in PATTERNS[metric].finditer(code):
                if metric == "rust_let_underscore" and propagates(code, match.end()):
                    continue
                found.append((metric, relative, code.count("\n", 0, match.start()) + 1))
    return found


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
    root = parser.parse_args().root.resolve()
    for metric, path, line in sites(root):
        print(f"{path}:{line}: {metric}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
