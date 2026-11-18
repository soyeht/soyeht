#!/usr/bin/env python3
"""Who reads each shared vector file, so a change to one runs only its readers (ADR 0089).

`contracts/vectors/` holds the cases that more than one implementation passes. A change to
a vector file can break only the tests that read it, so `gate_plan` selects the areas of the
readers listed in `VECTOR_READERS` instead of every code area. The rules that keep the table
true:

- a vector file that is not in `VECTOR_READERS` selects everything (the plan widens, it never
  skips);
- `test_gate_vectors` scans the sources and fails when a file reads a vector that its table
  entry does not list, when an entry lists a reader that no longer reads it, and when a vector
  file or a reader has no entry. The Python and shell checks (`contracts/*.py`, `scripts/`) are
  not readers here: the `always` steps run them whatever changed;
- the Swift tests that read a vector run as the fast `swift:test-vectors` step, whose filter
  is derived from the same scan (`swift-filter`), so a new vector test is in it.

A reader is a directory of `READER_AREAS` or an engine crate (`engine/<crate>`). A crate is
tested alone, without its reverse dependencies: a data file cannot break a crate that does not
read it, and the programs the suites start are not rebuilt for a change to a test input.
"""

from __future__ import annotations

import functools
import re
import subprocess
import sys
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
VECTORS = "contracts/vectors/"

# Reader directory -> the gate area its tests run in. `apps/shared` is the Swift vector step, not
# the whole `swift` area: that one builds the engine programs for the client's integration test.
READER_AREAS = {
    "apps/shared": "swift_vectors",
    "apps/desktop": "desktop",
    "apps/android": "android_core",
    "contracts/swift": "swift",
    "contracts/typescript": "typescript",
}

# vector file (without `.json`) -> the directories and engine crates whose tests read it.
VECTOR_READERS: dict[str, tuple[str, ...]] = {
    "agent-account": ("apps/desktop", "apps/shared"),
    "agent-attention": ("apps/desktop", "apps/shared"),
    "agent-cards": ("apps/desktop", "apps/shared"),
    "agent-transcript": ("apps/desktop", "apps/shared"),
    "attachments": ("apps/desktop", "apps/shared"),
    "avatar-mouth": ("apps/shared", "engine/soyeht-avatar"),
    "avatar-package": ("apps/shared", "engine/soyeht-avatar"),
    "editor": ("apps/desktop", "apps/shared"),
    "engine-proof": ("apps/desktop", "apps/shared", "engine/soyeht-engine", "engine/soyeht-protocol"),
    "file-paths": ("apps/desktop", "apps/shared"),
    "frames": ("contracts/swift", "contracts/typescript", "engine/soyeht-protocol"),
    "harness-picker": ("apps/desktop", "apps/shared"),
    "harness-switch": ("apps/android", "apps/desktop", "apps/shared"),
    "invalid-messages": ("contracts/swift", "contracts/typescript", "engine/soyeht-protocol"),
    "loop-panel": ("apps/desktop", "apps/shared"),
    "markdown": ("apps/android", "apps/desktop", "apps/shared"),
    "messages": ("contracts/swift", "contracts/typescript", "engine/soyeht-engine", "engine/soyeht-protocol"),
    "new-pane-form": ("apps/desktop", "apps/shared"),
    "pairing-approval": ("apps/desktop", "engine/soyeht-protocol"),
    "pairing-comparison": ("apps/desktop", "apps/shared", "engine/soyeht-protocol"),
    "pairing-link": ("apps/shared", "engine/soyeht-protocol"),
    "pane-close": ("apps/desktop", "apps/shared"),
    "pane-layout": ("apps/desktop", "apps/shared"),
    "pane-state-presentation": ("apps/desktop", "apps/shared", "engine/soyeht-protocol"),
    "permission-preview": ("apps/desktop", "apps/shared"),
    "presentation": ("apps/desktop", "apps/shared"),
    "project-page-origin": ("apps/desktop", "apps/shared", "engine/soyeht-protocol"),
    "protocol-version": ("apps/shared", "engine/soyeht-protocol"),
    "push-permission": ("apps/shared", "engine/soyeht-engine"),
    "readiness": ("apps/desktop", "apps/shared"),
    "reconnect": ("apps/desktop", "apps/shared"),
    "screenplay": ("apps/desktop", "apps/shared"),
    "slash-completion": ("apps/desktop", "apps/shared"),
    "speech-text": ("apps/shared",),
    "tasks": ("apps/desktop", "apps/shared"),
    "terminal-resize": ("apps/desktop", "apps/shared"),
    "update": ("apps/shared", "engine/soyeht-protocol"),
    "update-status": ("apps/desktop", "apps/shared"),
    "usage": ("apps/desktop", "apps/shared", "engine/soyeht-protocol"),
    "usage-presentation": ("apps/desktop", "apps/shared"),
    "voice-turns": ("apps/shared",),
    "web-links": ("apps/desktop", "apps/shared"),
    "web-read": ("apps/desktop", "apps/shared"),
    "workspaces": ("apps/desktop", "apps/shared"),
    "writing": ("apps/desktop", "apps/shared"),
}

SOURCE_SUFFIXES = {".rs", ".swift", ".ts", ".kt", ".kts"}
SKIPPED_DIRECTORIES = {"docs"}
COMMENT = re.compile(r"^\s*(//|/\*|\*)")
# The helpers that take a vector file's name: `ContractVector.load("x")`, `vector("x")`,
# `testing::cases("x", key)`, `read("x.json")`.
LOADERS = r"(?:load|cases|vector|vector_named|read)"
SWIFT_VECTOR_TESTS = "apps/shared/Tests/"
# A Swift test that names no vector but goes through the loader or the directory still reads one.
LOADER_HINTS = ("ContractVector.", VECTORS)


def vector_name(path: str) -> str | None:
    """`voice-turns` for `contracts/vectors/voice-turns.json`, else None."""
    if not path.startswith(VECTORS) or not path.endswith(".json") or "/" in path[len(VECTORS):]:
        return None
    return path[len(VECTORS):-len(".json")]


def readers_of(path: str) -> tuple[str, ...] | None:
    """The readers declared for a vector file, None when it is unknown (the plan widens)."""
    name = vector_name(path)
    return VECTOR_READERS.get(name) if name is not None else None


def reader_directory(path: str) -> str | None:
    """The reader a source file belongs to, None for a file that is no reader here."""
    if path.startswith("engine/") and path.count("/") >= 2:
        return "/".join(path.split("/")[:2])
    return next((root for root in READER_AREAS if path.startswith(root + "/")), None)


def code_lines(text: str) -> str:
    return "\n".join(line for line in text.splitlines() if not COMMENT.match(line))


def vectors_read(text: str, names: list[str]) -> set[str]:
    """The vector files a source reads: its path, `"name.json"` beside the word vector, or a loader call."""
    code = code_lines(text)
    mentions_vectors = "vector" in code.lower()
    found = set()
    for name in names:
        if name not in code:
            continue
        escaped = re.escape(name)
        path = rf"{VECTORS}{escaped}\.json|(?<![\w-]){escaped}\.json"
        if re.search(path, code) and mentions_vectors:
            found.add(name)
        elif re.search(rf"\b{LOADERS}\(\s*[\"']{escaped}(?:\.json)?[\"']", code):
            found.add(name)
    return found


def vector_names(root: Path = ROOT) -> list[str]:
    return sorted(path.stem for path in (root / VECTORS).glob("*.json"))


@functools.cache
def source_files(root: Path = ROOT) -> tuple[tuple[str, str], ...]:
    """(repository path, text) of every tracked or new source that can read a vector."""
    listed = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=root, text=True
    )
    return tuple(
        (relative, (root / relative).read_text(errors="replace"))
        for relative in sorted(set(listed.split("\0")) - {""})
        if PurePosixPath(relative).suffix in SOURCE_SUFFIXES
        and (root / relative).is_file()
        and not SKIPPED_DIRECTORIES.intersection(PurePosixPath(relative).parts)
        and not relative.startswith(VECTORS)
    )


def scan(root: Path = ROOT) -> dict[str, dict[str, set[str]]]:
    """vector name -> reader directory -> the source files that read it."""
    names = vector_names(root)
    found: dict[str, dict[str, set[str]]] = {name: {} for name in names}
    for path, text in source_files(root):
        reader = reader_directory(path) or "unclassified"
        for name in vectors_read(text, names):
            found[name].setdefault(reader, set()).add(path)
    return found


def swift_vector_tests(root: Path = ROOT) -> list[str]:
    """The Swift test files of the shared package that read a vector, by repository path."""
    names = vector_names(root)
    return sorted(
        path for path, text in source_files(root)
        if path.startswith(SWIFT_VECTOR_TESTS) and path.endswith(".swift")
        and (vectors_read(text, names) or any(hint in code_lines(text) for hint in LOADER_HINTS))
    )


def swift_filter(root: Path = ROOT) -> str:
    """A `swift test --filter` regular expression for those files.

    The Swift Testing ids of the package's tests end in `/File.swift:line:column`, so the file
    names select the tests that a file holds, whatever their own names are.
    """
    files = swift_vector_tests(root)
    if not files:
        raise SystemExit("gate_vectors: no Swift test reads a vector; fix the scan")
    names = "|".join(re.escape(Path(path).stem) for path in files)
    return rf"(?:{names})\.swift"


def main(argv: list[str]) -> int:
    if argv != ["swift-filter"]:
        print("usage: gate_vectors.py swift-filter", file=sys.stderr)
        return 2
    print(swift_filter())
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
