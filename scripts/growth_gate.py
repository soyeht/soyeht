#!/usr/bin/env python3
"""A change may add at most 400 net lines of test code, unless its pull request says why not.

    uv run python scripts/growth_gate.py [--base FULL_OID] [--event-file FILE]

"Net" is the test code of the working tree minus the test code of the merge base (the same base
as the other ratchets): a test deleted elsewhere in the change pays for a test added. More than
the limit needs a line in the pull request body, which the pre-merge gate reads from the pull
request and the post-merge gate from the squash commit message, as for `Verification change:`:

    Test growth allowance: 650 lines because <what the tests prove and why they need the room>

`agent_precheck.py` runs this check as one of the gate's `always` steps, so an author sees the
number before the push. What counts as test code is `lines_of_test_code`.

Not counted: `scripts/e2e*.py`, the test-support modules a crate keeps in `src/` (a `test_support.rs`,
a `testing.rs`) that no `#[cfg(test)] mod x;` declares, and every other file that is not in a test
location below. The three scripts that tell test code from production code (this one,
`check_quality.py` and `check_swallowed_errors.py`) do not share one list of paths yet; the extent
of an inline `#[cfg(test)]` item is the one of `check_swallowed_errors.test_item_spans`.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from bisect import bisect_left
from pathlib import Path, PurePosixPath

from check_swallowed_errors import blank_literals_and_comments, test_item_spans
from gate_base import BASE_BRANCH, BaseError, resolve_base
from gate_pr_text import body_lines, statements
from hardening_gate import (
    ROOT,
    SOURCE_SUFFIXES,
    CommitTree,
    Tree,
    WorkingTree,
    is_generated,
    mask,
    test_module_files,
)

LIMIT = 400
ALLOWANCE_LABEL = "Test growth allowance"
ALLOWANCE_SUBJECT = re.compile(r"(\d+)\s+lines?")
# The allowance may name a little more than the net: the net moves a few lines when main does.
SLACK_MINIMUM = 50
SLACK_FRACTION = 10
TOP_FILES = 5


def in_test_location(path: str) -> bool:
    """Whether the whole file is test code, by where it is and what it is called.

    Rust: a directory `tests` or `benches` (a crate's, or a `src/tests` module directory).
    Swift: a directory named `Tests` or ending in `Tests`, as every SwiftPM and Xcode test target.
    Python: `test_*.py`. Kotlin: a source set `test` or `androidTest`. TypeScript: `*.test.ts` or
    a directory `test`. A Rust file that is only a `#[cfg(test)] mod x;` is found with its source.
    """
    *directories, name = path.split("/")
    suffix = PurePosixPath(name).suffix
    if suffix == ".rs":
        return bool({"tests", "benches"} & set(directories))
    if suffix == ".swift":
        return any(part.endswith("Tests") for part in directories)
    if suffix == ".py":
        return name.startswith("test_")
    if suffix == ".kt":
        return bool({"test", "androidTest"} & set(directories))
    if suffix == ".ts":
        return name.endswith(".test.ts") or "test" in directories
    return False


def code_lines(source: str) -> int:
    return sum(1 for line in source.split("\n") if line.strip())


def rust_inline_test_lines(source: str) -> int:
    """The lines of `#[cfg(test)]` items, statements and fields, in a file that is otherwise production."""
    clean = blank_literals_and_comments(source)
    newlines = [found.start() for found in re.finditer("\n", clean)]
    lines = source.split("\n")
    counted: set[int] = set()
    for start, end in test_item_spans(clean):
        counted.update(range(bisect_left(newlines, start), bisect_left(newlines, max(start, end - 1)) + 1))
    return sum(1 for number in counted if number < len(lines) and lines[number].strip())


def lines_of_test_code(sources: dict[str, str]) -> dict[str, int]:
    """Lines of test code per file, for the files that hold any.

    A line is test code when it is a non-blank line of a source file in a test location, of a Rust
    module that only a `#[cfg(test)] mod x;` declares, or inside a `#[cfg(test)]` item. Generated
    files never count, and the shared vectors of `contracts/vectors/` are JSON, not source.
    `sources` maps paths to text.
    """
    rust = {
        path: mask(source)[0]
        for path, source in sources.items()
        if path.endswith(".rs") and not is_generated(path, source)
    }
    modules = test_module_files(rust)
    found: dict[str, int] = {}
    for path, source in sources.items():
        if PurePosixPath(path).suffix not in SOURCE_SUFFIXES or is_generated(path, source):
            continue
        if in_test_location(path) or path in modules:
            lines = code_lines(source)
        elif path in rust:
            lines = rust_inline_test_lines(source)
        else:
            lines = 0
        if lines:
            found[path] = lines
    return found


def tree_test_lines(tree: Tree) -> dict[str, int]:
    sources = {}
    for path in tree.paths():
        suffix = PurePosixPath(path).suffix
        if suffix in SOURCE_SUFFIXES and (suffix == ".rs" or in_test_location(path)):
            try:
                sources[path] = tree.read(path)
            except (OSError, UnicodeError, subprocess.CalledProcessError):
                continue  # a path the tree lists but cannot read is no source
    return lines_of_test_code(sources)


def covers(subject: str, net: int) -> str | None:
    """None when the allowance `subject` covers `net`, else why it does not."""
    granted = ALLOWANCE_SUBJECT.fullmatch(subject)
    if granted is None:
        return f"`{subject}` is not `<N> lines`"
    most = net + max(SLACK_MINIMUM, net // SLACK_FRACTION)
    if not net <= int(granted[1]) <= most:
        return f"{granted[1]} lines is not the net: write a number from {net} to {most}"
    return None


def check_growth(base: dict[str, int], head: dict[str, int], allowances: list[tuple[str, str]]) -> bool:
    """Whether the change grows test code past the limit without an allowance that covers it."""
    prefix = "check-test-growth:"
    net = sum(head.values()) - sum(base.values())
    print(f"{prefix} test code lines base={sum(base.values())} head={sum(head.values())} "
          f"net={net:+d} limit={LIMIT}")
    if net <= LIMIT:
        return False
    growth = {path: head.get(path, 0) - base.get(path, 0) for path in base.keys() | head.keys()}
    for path, delta in sorted(growth.items(), key=lambda item: (-item[1], item[0]))[:TOP_FILES]:
        print(f"{prefix} {delta:+d} {path}")
    for subject, reason in allowances:
        problem = covers(subject, net)
        if problem is None:
            print(f"{prefix} allowance of {subject}: {reason}")
            return False
        print(f"{prefix} an allowance line does not count: {problem}", file=sys.stderr)
    print(
        f"{prefix} this change adds {net} net lines of test code and the limit is {LIMIT}. Either "
        f"consolidate tests, or delete tests elsewhere in the change, until the net is {LIMIT} or "
        f"less; or add a line to the pull request body: `{ALLOWANCE_LABEL}: <N> lines because "
        f"<reason>`, with N from {net} to {net + max(SLACK_MINIMUM, net // SLACK_FRACTION)} and a "
        "reason of at least three words that says why these tests need the room (with "
        "agent_precheck.py, give the text with --pr-text FILE).",
        file=sys.stderr,
    )
    return True


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    parser.add_argument("--base", help=f"full base commit OID (default: merge-base of {BASE_BRANCH} and HEAD)")
    parser.add_argument("--event-file", type=Path)
    arguments = parser.parse_args()
    reference = resolve_base(ROOT, arguments.base)
    print(f"check-test-growth: base {reference}")
    allowances, unread = statements(body_lines(arguments.event_file), ALLOWANCE_LABEL)
    for line in unread:
        print(f"check-test-growth: allows nothing: {line!r}", file=sys.stderr)
    if check_growth(tree_test_lines(CommitTree(reference)), tree_test_lines(WorkingTree()), allowances):
        return 1
    print("check-test-growth: passed")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (BaseError, ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"check-test-growth: {error}", file=sys.stderr)
        sys.exit(1)
