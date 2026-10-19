"""The allowlist rules and the command line that the dead public declaration checks share.

`check_dead_pub.py` (Rust) and `check_dead_public_swift.py` (Swift) each find the declarations
that nothing refers to. What follows the finding is the same: a reviewed allowlist whose
entries carry a reason and fail once they match nothing, and the same command line.
"""

from __future__ import annotations

import argparse
import json
from collections.abc import Callable
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

Found = list[tuple[str, int, str]]


def allowlist_problems(
    found: Found, allowlist: dict[str, str], unlisted: str, noun: str
) -> list[str]:
    """Messages for the findings the allowlist does not cover and for its stale or blank entries.

    `unlisted` is the message of an uncovered finding, formatted with `path`, `line` and `name`.
    """
    keys = {f"{path}::{name}" for path, _, name in found}
    messages = [
        unlisted.format(path=path, line=line, name=name)
        for path, line, name in found
        if f"{path}::{name}" not in allowlist
    ]
    messages += [
        f"{key}: allowlist entry matches no {noun}; remove it"
        for key in sorted(set(allowlist) - keys)
    ]
    messages += [
        f"{key}: allowlist entry needs a reason"
        for key, reason in sorted(allowlist.items())
        if not reason.strip()
    ]
    return messages


def problems_of(
    find: Callable[[Path], Found], unlisted: str, noun: str
) -> Callable[[Path, dict[str, str]], list[str]]:
    """The `problems(root, allowlist)` of a check that finds its candidates with `find(root)`."""
    return lambda root, allowlist: allowlist_problems(find(root), allowlist, unlisted, noun)


def run(
    description: str,
    default_allowlist: Path,
    check: Callable[[Path, dict[str, str]], list[str]],
) -> int:
    """Parse the command line, print what `check(root, allowlist)` finds, and exit 1 if any."""
    parser = argparse.ArgumentParser(description=description)
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    parser.add_argument("--allowlist", type=Path, default=default_allowlist, help=argparse.SUPPRESS)
    arguments = parser.parse_args()
    allowlist = json.loads(arguments.allowlist.read_text(encoding="utf-8"))
    messages = check(arguments.root.resolve(), allowlist)
    for message in messages:
        print(message)
    return 1 if messages else 0
