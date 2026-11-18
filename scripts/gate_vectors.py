#!/usr/bin/env python3
"""Who reads each shared vector file, so a change to one runs only its readers (ADR 0089).

`contracts/vectors/` holds the cases that more than one implementation passes. A change to
a vector file can break only the tests that read it, so `gate_plan` selects the areas of the
readers that `readers` finds by scanning the sources. Nothing is listed by hand:

- a reader is a source that names a vector file by its path, by `"name.json"` beside the word
  vector, or by a call of a loader (`LOADERS`, the functions of `HELPERS`). A reader belongs to a
  group: an area directory (`READER_AREAS`), an engine crate (`engine/<crate>`), the `tests/`
  directory of a crate, or the gate's own scripts, which the `always` steps run;
- a vector file that no scan finds a reader for selects everything (the plan widens, it never
  skips), and so does a reader in no group;
- `test_gate_vectors` fails when a source holds `contracts/vectors` and names no vector, unless it
  is a helper of `HELPERS`: that is how a loader the scan cannot see is found;
- an engine crate that reads a vector is tested alone, without its reverse dependencies: a data
  file cannot break a crate that does not read it, and the engine programs are not rebuilt for
  a change to a test input. The `tests/` of a crate run in the fast tier only for
  `soyeht-protocol` (`engine:test-fast`); the others run after the merge;
- the Swift tests that read a vector, and the `protocol-vectors` program of `contracts/swift`,
  run as `swift:test-shared-changed`, whose filter is derived from the same scan (`swift-test`).
"""

from __future__ import annotations

import functools
import re
import subprocess
import sys
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
VECTORS = "contracts/vectors/"

# Reader group -> the gate area whose steps run its tests. `apps/shared` and `contracts/swift` are the
# Swift vector step, not the whole `swift` area: that one builds the engine programs for the client's
# integration test. The tests of `soyeht-protocol` are a pure crate, which `engine:test-fast` runs.
PROTOCOL_TESTS = "engine/soyeht-protocol/tests"
READER_AREAS = {
    "apps/shared": "swift_vectors",
    "contracts/swift": "swift_vectors",
    "apps/desktop": "desktop",
    "apps/android": "android_core",
    "contracts/typescript": "typescript",
    PROTOCOL_TESTS: "protocol_vectors",
}
# The gate's own scripts and the Python of the contracts: the `always` steps run them.
GATE = "gate"
UNCLASSIFIED = "unclassified"

# The sources that load a vector by a name they are given, and the functions that take it. The
# scan finds a read through one of these names; `test_gate_vectors` finds a source that holds
# `contracts/vectors` and is neither a named read nor in this list.
HELPERS = {
    "apps/desktop/crates/soyeht-client/src/testing.rs": ("vector", "cases"),
    "apps/shared/Tests/SoyehtClientTests/ContractVector.swift": ("load",),
    "apps/shared/Tests/SoyehtClientTests/SharedVectorsTests.swift": ("vector",),
    "engine/soyeht-protocol/tests/client_policy.rs": ("vector_named",),
    "engine/soyeht-avatar/src/tests/vectors/mod.rs": ("read",),
}
LOADERS = sorted({name for names in HELPERS.values() for name in names})

NOT_SOURCES = {".md", ".json", ".lock"}
MAX_SOURCE_BYTES = 2_000_000
COMMENT = re.compile(r"^\s*(//|/\*|\*|#)")
SWIFT_TESTS = "apps/shared/Tests/"


def vector_name(path: str) -> str | None:
    """`voice-turns` for `contracts/vectors/voice-turns.json`, else None."""
    if not path.startswith(VECTORS) or not path.endswith(".json") or "/" in path[len(VECTORS):]:
        return None
    return path[len(VECTORS):-len(".json")]


def reader_group(path: str) -> str:
    """The reader a source belongs to: where its tests run."""
    parts = path.split("/")
    if parts[0] == "engine" and len(parts) > 2:
        return "/".join(parts[:2]) + ("/tests" if parts[2] == "tests" else "")
    for group in READER_AREAS:
        if path.startswith(group + "/"):
            return group
    if parts[0] in ("scripts", ".github", ".githooks") or (parts[0] == "contracts" and len(parts) == 2):
        return GATE
    return UNCLASSIFIED


def code_lines(text: str) -> str:
    return "\n".join(line for line in text.splitlines() if not COMMENT.match(line))


def vectors_read(text: str, names: list[str]) -> set[str]:
    """The vector files a source reads: its path, `"name.json"` beside the word vector, or a loader call."""
    candidates = [name for name in names if name in text]
    if not candidates:
        return set()
    code = code_lines(text)
    mentions_vectors = "vector" in code.lower()
    found = set()
    for name in candidates:
        escaped = re.escape(name)
        path = rf"{VECTORS}{escaped}\.json|(?<![\w-]){escaped}\.json"
        call = rf"\b(?:{'|'.join(LOADERS)})\(\s*[\"']{escaped}(?:\.json)?[\"']"
        if (mentions_vectors and re.search(path, code)) or re.search(call, code):
            found.add(name)
    return found


def vector_names(root: Path = ROOT) -> list[str]:
    return sorted(path.stem for path in (root / VECTORS).glob("*.json"))


@functools.cache
def source_files(root: Path = ROOT) -> tuple[tuple[str, str], ...]:
    """(repository path, text) of every tracked or new text file that can read a vector."""
    listed = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=root, text=True
    )
    sources = []
    for relative in sorted(set(listed.split("\0")) - {""}):
        path = root / relative
        if (
            PurePosixPath(relative).suffix in NOT_SOURCES
            or relative.startswith(("docs/", VECTORS))
            or not path.is_file()
            or path.stat().st_size > MAX_SOURCE_BYTES
        ):
            continue
        data = path.read_bytes()
        if b"\0" not in data[:4096]:
            sources.append((relative, data.decode(errors="replace")))
    return tuple(sources)


@functools.cache
def scan(root: Path = ROOT) -> dict[str, dict[str, tuple[str, ...]]]:
    """vector name -> reader group -> the source files of that group that read it."""
    names = vector_names(root)
    found: dict[str, dict[str, list[str]]] = {name: {} for name in names}
    for path, text in source_files(root):
        group = reader_group(path)
        for name in vectors_read(text, names):
            found[name].setdefault(group, []).append(path)
    return {name: {group: tuple(files) for group, files in groups.items()} for name, groups in found.items()}


def readers(root: Path = ROOT) -> dict[str, tuple[str, ...]]:
    """vector name -> the reader groups that read it (an empty tuple: nothing reads it)."""
    return {name: tuple(sorted(groups)) for name, groups in scan(root).items()}


def swift_vector_tests(root: Path = ROOT) -> list[str]:
    """The Swift test files of the shared package that read a vector."""
    names = vector_names(root)
    return sorted(
        path for path, text in source_files(root)
        if path.startswith(SWIFT_TESTS) and path.endswith(".swift") and "@Test" in text
        and (vectors_read(text, names) or any(f in code_lines(text) for f in (VECTORS, "ContractVector.")))
    )


def swift_filter(root: Path = ROOT) -> str:
    """A `swift test --filter` regular expression for those files.

    The Swift Testing ids of the package's tests end in `/File.swift:line:column`, so the file
    names select the tests that a file holds, whatever their own names are.
    """
    names = "|".join(re.escape(PurePosixPath(path).stem) for path in swift_vector_tests(root))
    return rf"/(?:{names})\.swift:"


def protocol_tests(root: Path = ROOT) -> list[str]:
    """The integration test targets of `soyeht-protocol` that read a vector."""
    prefix = PROTOCOL_TESTS + "/"
    return sorted({
        PurePosixPath(path[len(prefix):]).parts[0].removesuffix(".rs")
        for groups in scan(root).values() for path in groups.get(PROTOCOL_TESTS, ())
    })


def tests_run(output: str) -> int:
    """The number of tests a Swift Testing run reports (0 when it says nothing)."""
    counts = re.findall(r"Test run with (\d+) tests?", output)
    return int(counts[-1]) if counts else 0


def swift_command(root: Path = ROOT) -> list[str]:
    return ["swift", "test", "--package-path", str(root / "apps/shared"), "--filter", swift_filter(root)]


def swift_test(root: Path = ROOT) -> int:
    """Run the Swift vector tests. A filter that matches nothing passes, so a short run fails."""
    files = swift_vector_tests(root)
    command = swift_command(root)
    lines = []
    with subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, text=True) as run:
        for line in run.stdout:
            sys.stdout.write(line)
            lines.append(line)
    if run.returncode:
        return run.returncode
    ran = tests_run("".join(lines))
    if ran < len(files):
        print(f"gate_vectors: {ran} Swift tests ran for {len(files)} test files that read a vector", file=sys.stderr)
        return 1
    return 0


def main(argv: list[str]) -> int:
    commands = {
        "swift-test": swift_test,
        "protocol-tests": lambda: print(" ".join(f"--test {name}" for name in protocol_tests())) or 0,
    }
    if len(argv) != 1 or argv[0] not in commands:
        print(f"usage: gate_vectors.py {{{'|'.join(commands)}}}", file=sys.stderr)
        return 2
    return commands[argv[0]]()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
