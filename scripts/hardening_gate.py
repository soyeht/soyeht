#!/usr/bin/env python3
"""Enforce hardening ratchets against a pull request base."""

from __future__ import annotations

import argparse
import ast
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Protocol

from gate_base import BASE_BRANCH, resolve_base


ROOT = Path(__file__).resolve().parents[1]
GENERATED_PARTS = {"generated.rs", "Generated.swift", "generated.ts", "schema.json"}
SOURCE_SUFFIXES = {".rs", ".swift", ".py", ".ts"}
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
    def __init__(self, revision: str):
        self.revision = revision

    def paths(self) -> list[str]:
        raw = git("ls-tree", "-rz", "--name-only", self.revision)
        return sorted(item.decode() for item in raw.split(b"\0") if item)

    def read(self, path: str) -> str:
        return git("show", f"{self.revision}:{path}").decode(errors="replace")


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


def git(*arguments: str) -> bytes:
    return subprocess.check_output(["git", *arguments], cwd=ROOT)


def line_number(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def mask(text: str, *, keep_strings: bool = False) -> tuple[str, list[str]]:
    """Blank comments and literals while preserving offsets and newlines."""
    output = list(text)
    strings: list[str] = []
    index = 0
    size = len(text)

    def blank(start: int, end: int) -> None:
        for position in range(start, end):
            if output[position] != "\n":
                output[position] = " "

    while index < size:
        if text.startswith("//", index):
            end = text.find("\n", index)
            end = size if end < 0 else end
            blank(index, end)
            index = end
            continue
        if text.startswith("/*", index):
            start = index
            index += 2
            depth = 1
            while index < size and depth:
                if text.startswith("/*", index):
                    depth += 1
                    index += 2
                elif text.startswith("*/", index):
                    depth -= 1
                    index += 2
                else:
                    index += 1
            blank(start, index)
            continue

        raw = re.match(r"(?:br|cr|r)(#+)?\"", text[index:])
        if raw:
            start = index
            hashes = raw.group(1) or ""
            content_start = index + raw.end()
            terminator = '"' + hashes
            end_at = text.find(terminator, content_start)
            index = size if end_at < 0 else end_at + len(terminator)
            strings.append(text[content_start:end_at if end_at >= 0 else size])
            if not keep_strings:
                blank(start, index)
            continue

        prefix = 1 if index + 1 < size and text[index] in "bc" and text[index + 1] == '"' else 0
        if text[index + prefix:index + prefix + 1] == '"':
            start = index
            index += prefix + 1
            content_start = index
            escaped = False
            while index < size:
                character = text[index]
                if character == '"' and not escaped:
                    break
                if character == "\\" and not escaped:
                    escaped = True
                else:
                    escaped = False
                index += 1
            strings.append(text[content_start:index])
            index = min(size, index + 1)
            if not keep_strings:
                blank(start, index)
            continue

        if text[index] == "'":
            closing = index + 1
            escaped = False
            while closing < min(size, index + 8):
                character = text[closing]
                if character == "'" and not escaped:
                    blank(index, closing + 1)
                    index = closing + 1
                    break
                if character == "\\" and not escaped:
                    escaped = True
                else:
                    escaped = False
                closing += 1
            else:
                index += 1
            continue
        index += 1
    return "".join(output), strings


def matching_brace(source: str, opening: int) -> int | None:
    depth = 0
    for index in range(opening, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return index
    return None


def is_engine_production_source(path: str) -> bool:
    """A crate's `src/` tree; `tests/` and `benches/` are never production."""
    parts = path.split("/")
    return (
        len(parts) >= 4
        and parts[0] == "engine"
        and parts[2] == "src"
        and path.endswith(".rs")
        and parts[-1] not in GENERATED_PARTS
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


def test_module_files(sources: dict[str, str]) -> set[str]:
    """Files compiled only for tests: `#[cfg(test)] mod x;` and their submodules."""
    pending = [
        candidate
        for path, source in sources.items()
        for found in TEST_MODULE_FILE.finditer(mask(source)[0])
        for candidate in child_module_paths(path, found.group(1))
    ]
    test_files: set[str] = set()
    while pending:
        path = pending.pop()
        if path not in sources or path in test_files:
            continue
        test_files.add(path)
        for found in ANY_MODULE_FILE.finditer(mask(sources[path])[0]):
            pending.extend(child_module_paths(path, found.group(1)))
    return test_files


def engine_production_panics(sources: dict[str, str]) -> int:
    """The one panic counter: unwrap, expect and panic! in engine production code.

    `sources` maps repository paths to text. Crate `tests/` and `benches/`,
    inline `#[cfg(test)]` items and test modules kept in their own files are
    excluded.
    """
    production = {
        path: source for path, source in sources.items() if is_engine_production_source(path)
    }
    excluded = test_module_files(production)
    return sum(
        len(PANIC_CALL.findall(without_cfg_test(source)))
        for path, source in production.items()
        if path not in excluded
    )


def without_cfg_test(source: str) -> str:
    clean, _ = mask(source)
    output = list(clean)
    for found in list(CFG_TEST_ATTRIBUTE.finditer(clean)):
        opening = clean.find("{", found.end())
        if opening < 0:
            continue
        declaration = clean.find(";", found.end(), opening)
        if declaration >= 0:
            # `mod x;` or `use ...;`: a module file is handled by test_module_files.
            continue
        closing = matching_brace(clean, opening)
        if closing is None:
            continue
        for index in range(found.start(), closing + 1):
            if output[index] != "\n":
                output[index] = " "
    return "".join(output)


def brace_functions(path: str, source: str, pattern: re.Pattern[str]) -> list[Function]:
    clean, _ = mask(source)
    functions = []
    for found in pattern.finditer(clean):
        opening = clean.find("{", found.end() - 1)
        if opening < 0:
            continue
        semicolon = clean.find(";", found.end() - 1, opening)
        if semicolon >= 0:
            continue
        closing = matching_brace(clean, opening)
        if closing is None:
            continue
        name = next((group for group in found.groups() if group), "anonymous")
        if name in {"if", "else", "for", "while", "switch", "catch"}:
            continue
        first = line_number(clean, found.start())
        functions.append(Function(path, name, first, line_number(clean, closing) - first + 1))
    return functions


def python_functions(path: str, source: str) -> list[Function]:
    try:
        tree = ast.parse(source)
    except SyntaxError:
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
        try:
            tree = ast.parse(source)
        except SyntaxError:
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
    try:
        tree = ast.parse(source)
    except SyntaxError:
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
        if path.endswith("Cargo.toml") and path != "engine/soyeht-store/Cargo.toml":
            if re.search(r"(?m)^\s*rusqlite(?:\.|\s*=)", source):
                manifests.add(path)
        if is_engine_production_source(path):
            if not path.startswith("engine/soyeht-store/"):
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
