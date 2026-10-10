#!/usr/bin/env python3
"""Enforce hardening ratchets against a pull request base."""

from __future__ import annotations

import argparse
import ast
import functools
import json
import re
import subprocess
import sys
from bisect import bisect_left
from dataclasses import dataclass
from pathlib import Path
from typing import Protocol

from gate_base import BASE_BRANCH, resolve_base


ROOT = Path(__file__).resolve().parents[1]
GENERATED_PARTS = {"generated.rs", "Generated.swift", "generated.ts", "schema.json"}
SOURCE_SUFFIXES = {".rs", ".swift", ".py", ".ts"}
# The one SQLite use outside soyeht-store: reading other applications' databases (the Cursor
# app's state, browser cookie stores) read-only and immutable, never Soyeht's own data (ADR 0077).
FOREIGN_DATABASE_READERS = frozenset({
    "engine/soyeht-usage/Cargo.toml",
    "engine/soyeht-usage/src/foreign_db.rs",
})
SQL = re.compile(
    r"(?is)\b(?:select\b.{0,400}\bfrom|insert(?:\s+or\s+\w+)?\s+into|"
    r"update\s+[\w.`\"']+\s+set|delete\s+from|create\s+table|alter\s+table|"
    r"pragma\s+\w+)"
)
PANIC_CALL = re.compile(r"(?:\.\s*(?:unwrap|expect)\s*\(|\bpanic\s*!\s*\()")
RUST_FUNCTION = re.compile(
    r"\b(?:pub(?:\s*\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?"
    r"(?:unsafe\s+)?(?:extern\s+(?:\"[^\"]*\"\s+)?)?fn\s+([A-Za-z_][A-Za-z0-9_]*)"
)
SWIFT_FUNCTION = re.compile(
    r"\b(?:func\s+([A-Za-z_][A-Za-z0-9_]*)|init\s*\(|deinit\s*\{|subscript\s*\()"
)
TS_FUNCTION = re.compile(
    r"\bfunction\s+([A-Za-z_$][A-Za-z0-9_$]*)|"
    r"\b(?:async\s+)?([A-Za-z_$][A-Za-z0-9_$]*)\s*\([^;{}]*\)\s*(?::[^={]+)?\s*\{"
)


class Tree(Protocol):
    def paths(self) -> list[str]: ...
    def read(self, path: str) -> str: ...


class WorkingTree:
    def paths(self) -> list[str]:
        raw = git("ls-files", "-z", "--cached", "--others", "--exclude-standard")
        return sorted(item.decode() for item in raw.split(b"\0") if item)

    def read(self, path: str) -> str:
        return (ROOT / path).read_text(encoding="utf-8", errors="replace")


class CommitTree:
    """A commit's files, all read through one `git cat-file --batch` process."""

    def __init__(self, revision: str):
        self.revision = revision
        self._blob_ids: dict[str, str | None] | None = None
        self._contents: dict[str, bytes] | None = None

    def _listing(self) -> dict[str, str | None]:
        """Path to blob id; an entry that is not a blob (a submodule) maps to None."""
        if self._blob_ids is None:
            self._blob_ids = {}
            for item in git("ls-tree", "-rz", self.revision).split(b"\0"):
                if item:
                    header, _, name = item.partition(b"\t")
                    _, kind, blob_id = header.decode().split()
                    self._blob_ids[name.decode()] = blob_id if kind == "blob" else None
        return self._blob_ids

    def paths(self) -> list[str]:
        return sorted(self._listing())

    def _load(self) -> dict[str, bytes]:
        blob_ids = self._listing()
        wanted = sorted({blob_id for blob_id in blob_ids.values() if blob_id})
        request = "".join(f"{blob_id}\n" for blob_id in wanted).encode()
        raw = git("cat-file", "--batch", stdin=request)
        by_id: dict[str, bytes] = {}
        cursor = 0
        for _ in wanted:
            end = raw.index(b"\n", cursor)
            blob_id, kind, size = raw[cursor:end].decode().split()
            if kind != "blob":
                raise OSError(f"{blob_id} is {kind}, not a blob")
            by_id[blob_id] = raw[end + 1:end + 1 + int(size)]
            cursor = end + 1 + int(size) + 1
        return {path: by_id[blob_id] for path, blob_id in blob_ids.items() if blob_id}

    def read(self, path: str) -> str:
        if self._contents is None:
            self._contents = self._load()
        if path not in self._contents:
            raise OSError(f"{path} is not a file in {self.revision}")
        return self._contents[path].decode(errors="replace")


@dataclass(frozen=True)
class Function:
    path: str
    name: str
    first_line: int
    lines: int


@dataclass(frozen=True)
class Metrics:
    sql_manifests: frozenset[str]
    sql_sources: frozenset[str]
    sql_statements: int
    panic_calls: int
    functions: tuple[Function, ...]
    tests: int
    acceptances: int
    gate_steps: int
    ignored_tests: int


def git(*arguments: str, stdin: bytes | None = None) -> bytes:
    return subprocess.check_output(["git", *arguments], cwd=ROOT, input=stdin)


LIFETIME = re.compile(r"'[A-Za-z_]\w*(?![\w'])")
# The next place `mask` has to look at; the text between two tokens is plain code.
MASK_TOKEN = re.compile(r"//|/\*|(?:br|cr|r)(#*)\"|[bc]?\"|'")
BLOCK_COMMENT_EDGE = re.compile(r"/\*|\*/")
STRING_BODY = re.compile(r'[^"\\]*(?:\\.[^"\\]*)*', re.DOTALL)
NOT_NEWLINE = re.compile(r"[^\n]")
NEWLINE = re.compile(r"\n")
BRACE = re.compile(r"[{}]")


def blanked(text: str) -> str:
    """The text with every character but the newlines replaced by a space."""
    return " " * len(text) if "\n" not in text else NOT_NEWLINE.sub(" ", text)


def block_comment_end(text: str, start: int) -> int:
    """The offset after the nestable block comment that opens at `start`."""
    depth = 1
    cursor = start + 2
    while depth:
        edge = BLOCK_COMMENT_EDGE.search(text, cursor)
        if edge is None:
            return len(text)
        depth += 1 if edge.group() == "/*" else -1
        cursor = edge.end()
    return cursor


def character_literal_end(text: str, start: int) -> int | None:
    """The offset after the short `'x'` literal that opens at `start`, or None."""
    escaped = False
    for closing in range(start + 1, min(len(text), start + 8)):
        character = text[closing]
        if character == "'" and not escaped:
            return closing + 1
        escaped = character == "\\" and not escaped
    return None


@functools.lru_cache(maxsize=8)
def mask(
    text: str, *, keep_strings: bool = False, lifetimes: bool = False
) -> tuple[str, list[str]]:
    """Blank comments and literals while preserving offsets and newlines.

    With `lifetimes`, a Rust `'a` is kept as code instead of opening a character literal.
    One pass jumps from token to token, so the cost is linear in the text. One file is masked
    by several counters in a row, so the result is cached; callers must not modify it.
    """
    pieces: list[str] = []
    strings: list[str] = []
    copied = 0
    index = 0
    size = len(text)

    def blank(start: int, end: int) -> None:
        nonlocal copied
        pieces.append(text[copied:start])
        pieces.append(blanked(text[start:end]))
        copied = end

    while (token := MASK_TOKEN.search(text, index)) is not None:
        index = token.start()
        opening = token.group()
        if opening == "//":
            end = text.find("\n", index)
            end = size if end < 0 else end
            blank(index, end)
            index = end
        elif opening == "/*":
            end = block_comment_end(text, index)
            blank(index, end)
            index = end
        elif opening == "'":
            lifetime = LIFETIME.match(text, index) if lifetimes else None
            literal_end = None if lifetime else character_literal_end(text, index)
            if lifetime:
                index = lifetime.end()
            elif literal_end is None:
                index += 1
            else:
                blank(index, literal_end)
                index = literal_end
        else:
            content_start = token.end()
            if token.group(1) is not None:
                terminator = '"' + token.group(1)
                end_at = text.find(terminator, content_start)
                content_end = size if end_at < 0 else end_at
                end = size if end_at < 0 else end_at + len(terminator)
            else:
                content_end = STRING_BODY.match(text, content_start).end()
                closed = content_end < size and text[content_end] == '"'
                content_end = content_end if closed else size
                end = content_end + 1 if closed else size
            strings.append(text[content_start:content_end])
            if not keep_strings:
                blank(index, end)
            index = end
    pieces.append(text[copied:])
    return "".join(pieces), strings


def brace_pairs(source: str) -> dict[int, int]:
    """Map the offset of every `{` to the offset of its matching `}`, in one pass.

    An unmatched `{` has no entry.
    """
    pairs: dict[int, int] = {}
    opened: list[int] = []
    for found in BRACE.finditer(source):
        if found.group() == "{":
            opened.append(found.start())
        elif opened:
            pairs[opened.pop()] = found.start()
    return pairs


@functools.lru_cache(maxsize=8)
def parse_python(source: str) -> ast.Module | None:
    """The syntax tree of a Python source, or None when it does not parse."""
    try:
        return ast.parse(source)
    except SyntaxError:
        return None


# Where Rust crates live: `<root>/<crate>/src/...`.
RUST_CRATE_ROOTS = (("engine",), ("apps", "desktop", "crates"))


def is_rust_production_source(path: str) -> bool:
    """A crate's `src/` tree; `tests/` and `benches/` are never production."""
    parts = path.split("/")
    return (
        path.endswith(".rs")
        and parts[-1] not in GENERATED_PARTS
        and any(
            tuple(parts[: len(root)]) == root
            and len(parts) >= len(root) + 3
            and parts[len(root) + 1] == "src"
            for root in RUST_CRATE_ROOTS
        )
    )


# `#[cfg(test)]` or `#[cfg(all(test, ...))]`, on source already masked by `mask`.
CFG_TEST = (
    r"#\s*\[\s*cfg\s*\(\s*(?:test|all\s*\((?:[^()]|\([^()]*\))*?\btest\b"
    r"(?:[^()]|\([^()]*\))*\))\s*\)\s*\]"
)
CFG_TEST_ATTRIBUTE = re.compile(CFG_TEST)
MODULE_FILE = r"(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;"
TEST_MODULE_FILE = re.compile(CFG_TEST + r"(?:\s*#\s*\[[^\]]*\])*\s*" + MODULE_FILE)
ANY_MODULE_FILE = re.compile(r"\b" + MODULE_FILE)


def child_module_paths(path: str, name: str) -> tuple[str, str]:
    parent = Path(path)
    directory = parent.parent
    if parent.name not in {"lib.rs", "main.rs", "mod.rs"}:
        directory = directory / parent.stem
    return ((directory / f"{name}.rs").as_posix(), (directory / name / "mod.rs").as_posix())


def test_module_files(masked: dict[str, str]) -> set[str]:
    """Files compiled only for tests: `#[cfg(test)] mod x;` and their submodules.

    `masked` maps repository paths to source already passed through `mask`.
    """
    pending = [
        candidate
        for path, clean in masked.items()
        for found in TEST_MODULE_FILE.finditer(clean)
        for candidate in child_module_paths(path, found.group(1))
    ]
    test_files: set[str] = set()
    while pending:
        path = pending.pop()
        if path not in masked or path in test_files:
            continue
        test_files.add(path)
        for found in ANY_MODULE_FILE.finditer(masked[path]):
            pending.extend(child_module_paths(path, found.group(1)))
    return test_files


def engine_production_panics(sources: dict[str, str]) -> int:
    """The one panic counter: unwrap, expect and panic! in engine and desktop production code.

    `sources` maps repository paths to text. Crate `tests/` and `benches/`,
    inline `#[cfg(test)]` items and test modules kept in their own files are
    excluded.
    """
    masked = {
        path: mask(source)[0]
        for path, source in sources.items()
        if is_rust_production_source(path)
    }
    excluded = test_module_files(masked)
    return sum(
        len(PANIC_CALL.findall(without_cfg_test(clean)))
        for path, clean in masked.items()
        if path not in excluded
    )


def without_cfg_test(clean: str) -> str:
    """Blank every `#[cfg(test)]` item that has a body; `clean` is already masked."""
    pairs = brace_pairs(clean)
    pieces: list[str] = []
    copied = 0
    for found in CFG_TEST_ATTRIBUTE.finditer(clean):
        opening = clean.find("{", found.end())
        if opening < 0:
            continue
        declaration = clean.find(";", found.end(), opening)
        if declaration >= 0:
            # `mod x;` or `use ...;`: a module file is handled by test_module_files.
            continue
        closing = pairs.get(opening)
        if closing is None or closing < copied:
            continue
        start = max(found.start(), copied)
        pieces.append(clean[copied:start])
        pieces.append(blanked(clean[start:closing + 1]))
        copied = closing + 1
    pieces.append(clean[copied:])
    return "".join(pieces)


SWIFT_CONTINUES_AFTER_LINE = re.compile(r"\s*(?:\{|->|,|&|where\b|throws\b|rethrows\b|async\b)")
SWIFT_CONTINUES_BEFORE_LINE = re.compile(r"(?:->|,|&|:|==|\bwhere)$")


def skip_generic_parameters(clean: str, index: int) -> int:
    """Return the index after a `<...>` list at `index`, ignoring `->` and `=>` arrows."""
    cursor = index
    while clean[cursor:cursor + 1].isspace():
        cursor += 1
    if clean[cursor:cursor + 1] != "<":
        return index
    depth = 0
    while cursor < len(clean):
        character = clean[cursor]
        if character == "<":
            depth += 1
        elif character == ">" and clean[cursor - 1] not in "-=":
            depth -= 1
            if depth == 0:
                return cursor + 1
        cursor += 1
    return index


def swift_line_continues(clean: str, newline: int) -> bool:
    """Whether a declaration signature goes on after the newline at `newline`."""
    if SWIFT_CONTINUES_AFTER_LINE.match(clean, newline + 1):
        return True
    line_start = clean.rfind("\n", 0, newline) + 1
    return bool(SWIFT_CONTINUES_BEFORE_LINE.search(clean[line_start:newline].rstrip()))


# The characters `body_opening` has to look at; everything between them is skipped.
SIGNATURE_STOP = re.compile(r"[(\[)\]{};]")
SWIFT_SIGNATURE_STOP = re.compile(r"[(\[)\]{};\n]")


def body_opening(clean: str, start: int, *, swift: bool) -> int | None:
    """Find the `{` that opens the body of the declaration signature at `start`.

    Nesting is tracked so braces in default-argument closures and semicolons in
    array types do not count. A signature that ends first has no body: a Rust or
    TypeScript `;`, a closing `}` of the enclosing type, or, in Swift, a newline
    that does not continue the signature (a protocol requirement).
    """
    depth = 0
    stops = SWIFT_SIGNATURE_STOP if swift else SIGNATURE_STOP
    for found in stops.finditer(clean, skip_generic_parameters(clean, start)):
        character = found.group()
        if character in "([":
            depth += 1
        elif character in ")]":
            depth -= 1
            if depth < 0:
                return None
        elif depth == 0:
            if character == "{":
                return found.start()
            if character in ";}":
                return None
            if character == "\n" and not swift_line_continues(clean, found.start()):
                return None
    return None


def brace_functions(path: str, source: str, pattern: re.Pattern[str]) -> list[Function]:
    clean, _ = mask(source, lifetimes=pattern is RUST_FUNCTION)
    swift = pattern is SWIFT_FUNCTION
    pairs = brace_pairs(clean)
    newlines = [found.start() for found in NEWLINE.finditer(clean)]

    def line_number(offset: int) -> int:
        return bisect_left(newlines, offset) + 1

    functions = []
    for found in pattern.finditer(clean):
        # `init(` and `deinit {` end on the opening delimiter; a name ends before it.
        start = found.end() - 1 if clean[found.end() - 1] in "({" else found.end()
        opening = body_opening(clean, start, swift=swift)
        if opening is None:
            continue
        closing = pairs.get(opening)
        if closing is None:
            continue
        name = next((group for group in found.groups() if group), "anonymous")
        if name in {"if", "else", "for", "while", "switch", "catch"}:
            continue
        first = line_number(found.start())
        functions.append(Function(path, name, first, line_number(closing) - first + 1))
    return functions


def python_functions(path: str, source: str) -> list[Function]:
    tree = parse_python(source)
    if tree is None:
        return []
    result = []
    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.end_lineno:
            result.append(Function(path, node.name, node.lineno, node.end_lineno - node.lineno + 1))
    return result


def source_functions(path: str, source: str) -> list[Function]:
    suffix = Path(path).suffix
    if suffix == ".rs":
        return brace_functions(path, source, RUST_FUNCTION)
    if suffix == ".swift":
        return brace_functions(path, source, SWIFT_FUNCTION)
    if suffix == ".ts":
        return brace_functions(path, source, TS_FUNCTION)
    if suffix == ".py":
        return python_functions(path, source)
    return []


def count_tests(path: str, source: str) -> int:
    suffix = Path(path).suffix
    if suffix == ".rs":
        clean, _ = mask(source)
        return len(re.findall(r"#\s*\[\s*(?:(?:tokio|test_case)::)?test(?:\s*\([^]]*\))?\s*\]", clean))
    if suffix == ".swift":
        clean, _ = mask(source)
        return len(re.findall(r"(?m)^\s*@Test(?:\s|\()", clean))
    if suffix == ".py":
        tree = parse_python(source)
        if tree is None:
            return 0
        return sum(
            isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name.startswith("test_")
            for node in ast.walk(tree)
        )
    if suffix == ".ts" and (path.endswith(".test.ts") or "/test/" in path):
        clean, _ = mask(source)
        return len(re.findall(r"\b(?:test|it)\s*\(", clean))
    return 0


def count_ignored_tests(path: str, source: str) -> int:
    if Path(path).suffix != ".rs":
        return 0
    clean, _ = mask(source)
    attributes = re.findall(r"#\s*\[([^]]*)\]", clean)
    return sum(
        bool(re.fullmatch(r"\s*ignore(?:\s*=.*)?\s*", attribute))
        or bool(
            re.match(r"\s*cfg_attr\s*\(", attribute)
            and re.search(r"\bignore\b", attribute)
        )
        for attribute in attributes
    )


def count_gate_steps(source: str) -> int:
    commands = (
        r"(?:python3|uv\s+run\s+python)\s+(?:-m\s+unittest|scripts/|contracts/)",
        r"cargo\s+(?:fmt|clippy|build|test)\b",
        r"bash\s+scripts/",
        r"swift\s+(?:format|test)\b",
        r"npm\s+(?:ci|run|test)\b",
    )
    commands_only = "\n".join(
        line for line in source.splitlines() if not line.lstrip().startswith("#")
    )
    return sum(len(re.findall(command, commands_only)) for command in commands)


def count_acceptances(source: str) -> int:
    tree = parse_python(source)
    if tree is None:
        return 0
    count = 0
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Attribute):
            continue
        if node.func.attr != "append" or not node.args or not isinstance(node.args[0], ast.Dict):
            continue
        keys = {
            key.value for key in node.args[0].keys
            if isinstance(key, ast.Constant) and isinstance(key.value, str)
        }
        if "name" in keys and "passed" in keys:
            count += 1
    return count


def metrics(tree: Tree) -> Metrics:
    manifests: set[str] = set()
    sql_sources: set[str] = set()
    functions: list[Function] = []
    engine_sources: dict[str, str] = {}
    tests = ignored_tests = acceptances = gate_steps = sql_statements = 0
    for path in tree.paths():
        try:
            source = tree.read(path)
        except (OSError, UnicodeError, subprocess.CalledProcessError):
            continue
        sql_allowed = path in FOREIGN_DATABASE_READERS
        if (path.endswith("Cargo.toml") and path != "engine/soyeht-store/Cargo.toml"
                and not sql_allowed):
            if re.search(r"(?m)^\s*rusqlite(?:\.|\s*=)", source):
                manifests.add(path)
        if is_rust_production_source(path):
            if not path.startswith("engine/soyeht-store/") and not sql_allowed:
                _, strings = mask(source, keep_strings=True)
                matches = sum(len(SQL.findall(value)) for value in strings)
                if matches:
                    sql_sources.add(path)
                    sql_statements += matches
            engine_sources[path] = source
        generated = Path(path).name in GENERATED_PARTS or source.startswith("// Generated from ")
        if Path(path).suffix in SOURCE_SUFFIXES and not generated:
            functions.extend(source_functions(path, source))
            tests += count_tests(path, source)
            ignored_tests += count_ignored_tests(path, source)
        if path.startswith("scripts/e2e") and path.endswith(".py"):
            acceptances += count_acceptances(source)
        if path == "scripts/check_all.sh":
            gate_steps = count_gate_steps(source)
    return Metrics(
        frozenset(manifests),
        frozenset(sql_sources),
        sql_statements,
        engine_production_panics(engine_sources),
        tuple(functions),
        tests,
        acceptances,
        gate_steps,
        ignored_tests,
    )


def waiver(event_file: Path | None) -> bool:
    if event_file is None:
        return False
    event = json.loads(event_file.read_text())
    body = (event.get("pull_request", {}).get("body") or "").splitlines()
    return any(re.fullmatch(r"Verification change:\s*\S.*", line) for line in body)


def refuse_increase(label: str, base: int, head: int) -> bool:
    print(f"{label}: base={base} head={head}")
    if head > base:
        print(f"{label}: ratchet increased", file=sys.stderr)
        return True
    return False


def check_sql(base: Metrics, head: Metrics) -> bool:
    base_items = base.sql_manifests | base.sql_sources
    head_items = head.sql_manifests | head.sql_sources
    added = sorted(head_items - base_items)
    base_count = len(base.sql_manifests) + base.sql_statements
    head_count = len(head.sql_manifests) + head.sql_statements
    print(
        f"check-no-sql-outside-store: files base={len(base_items)} head={len(head_items)} "
        f"violations base={base_count} head={head_count}"
    )
    for path in sorted(head_items):
        print(f"check-no-sql-outside-store: remaining {path}")
    for path in added:
        print(f"check-no-sql-outside-store: new violation {path}", file=sys.stderr)
    return bool(added) or len(head_items) > len(base_items) or head_count > base_count


def check_panic(base: Metrics, head: Metrics) -> bool:
    return refuse_increase("check-no-panic-in-production", base.panic_calls, head.panic_calls)


def check_verification(base: Metrics, head: Metrics, allowed: bool) -> bool:
    failed = False
    for label, before, after in (
        ("tests", base.tests, head.tests),
        ("acceptances", base.acceptances, head.acceptances),
        ("gate steps", base.gate_steps, head.gate_steps),
    ):
        print(f"check-verification-not-weakened: {label} base={before} head={after}")
        if after < before and not allowed:
            print(
                f"check-verification-not-weakened: {label} dropped without a Verification change line",
                file=sys.stderr,
            )
            failed = True
    print(
        "check-verification-not-weakened: ignored tests "
        f"base={base.ignored_tests} head={head.ignored_tests}"
    )
    if head.ignored_tests > base.ignored_tests and not allowed:
        print(
            "check-verification-not-weakened: ignored tests increased without a Verification change line",
            file=sys.stderr,
        )
        failed = True
    return failed


def check_lengths(base: Metrics, head: Metrics) -> bool:
    base_long = [function for function in base.functions if function.lines > 120]
    head_long = [function for function in head.functions if function.lines > 120]
    base_critical = [function for function in base_long if function.lines > 200]
    head_critical = [function for function in head_long if function.lines > 200]
    for function in sorted(head_long, key=lambda item: (-item.lines, item.path, item.first_line)):
        level = "error" if function.lines > 200 else "warning"
        print(
            f"check-function-length: {level} {function.path}:{function.first_line} "
            f"{function.name} has {function.lines} lines"
        )
    failed = refuse_increase("check-function-length >120", len(base_long), len(head_long))
    failed |= refuse_increase("check-function-length >200", len(base_critical), len(head_critical))
    base_by_identity = {
        (function.path, function.name): function.lines for function in base.functions
    }
    for function in head_long:
        before = base_by_identity.get((function.path, function.name), 120)
        if function.lines > before:
            print(
                f"check-function-length: {function.path}:{function.first_line} "
                f"{function.name} grew from {before} to {function.lines}",
                file=sys.stderr,
            )
            failed = True
    base_max = max((function.lines for function in base.functions), default=0)
    head_max = max((function.lines for function in head.functions), default=0)
    print(f"check-function-length: max base={base_max} head={head_max}")
    if head_max > max(200, base_max):
        print("check-function-length: maximum grew above the existing ceiling", file=sys.stderr)
        failed = True
    return failed


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "check",
        choices=(
            "check-no-sql-outside-store",
            "check-no-panic-in-production",
            "check-verification-not-weakened",
            "check-function-length",
        ),
    )
    parser.add_argument(
        "--base",
        help=f"full base commit OID (default: merge-base of {BASE_BRANCH} and HEAD)",
    )
    parser.add_argument("--event-file", type=Path)
    return parser.parse_args()


def main() -> int:
    arguments = parse_args()
    reference = resolve_base(ROOT, arguments.base)
    origin = "--base" if arguments.base else f"merge-base with {BASE_BRANCH}"
    print(f"{arguments.check}: base {reference} ({origin})")
    base = metrics(CommitTree(reference))
    head = metrics(WorkingTree())
    checks = {
        "check-no-sql-outside-store": lambda: check_sql(base, head),
        "check-no-panic-in-production": lambda: check_panic(base, head),
        "check-verification-not-weakened": lambda: check_verification(
            base, head, waiver(arguments.event_file)
        ),
        "check-function-length": lambda: check_lengths(base, head),
    }
    failed = checks[arguments.check]()
    if failed:
        return 1
    print(f"{arguments.check}: passed")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (ValueError, OSError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        print(f"hardening gate: {error}", file=sys.stderr)
        sys.exit(1)
