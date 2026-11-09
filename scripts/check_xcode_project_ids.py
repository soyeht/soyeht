#!/usr/bin/env python3
"""Fail when an Xcode project has two objects with the same id, or a dangling build file.

Xcode reads `project.pbxproj` into a dictionary, so a second object with the id of an earlier
one silently replaces it. Two pull requests that each add a file by hand with the same id
leave one source file out of the build, and only the Mac app build in the full tier notices.
The same parse makes a `PBXBuildFile` whose `fileRef` names no object build nothing, silently.

The check reads the `objects` dictionary of every `apps/*/*.xcodeproj/project.pbxproj` and
fails on an object id defined more than once there, and on a `PBXBuildFile` whose `fileRef`
is not an object of that dictionary. It exits 0 with a line `ok` otherwise.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

PROJECTS = "apps/*/*.xcodeproj/project.pbxproj"

ID = r"[0-9A-Fa-f]{24}"
OBJECTS = re.compile(r'^\s*"?objects"?\s*=\s*\{\s*$')
# `"<id>" = {`, `<id> /* Name */ = {`, or the quoted form of the second.
ENTRY = re.compile(rf'^\s*(?:"({ID})"|({ID}))\s*(?:/\*.*?\*/\s*)?=\s*\{{\s*$')
ISA = re.compile(r'^\s*"?isa"?\s*=\s*"?([A-Za-z]+)"?\s*;')
FILE_REF = re.compile(rf'^\s*"?fileRef"?\s*=\s*"?({ID})"?\s*;')
COMMENT = re.compile(r"/\*.*?\*/")


def depth_change(line: str) -> int:
    """The net brace depth a line adds, with its `/* ... */` comments left out."""
    code = COMMENT.sub("", line)
    return code.count("{") - code.count("}")


def scan(text: str) -> tuple[dict[str, list[int]], list[tuple[str, int, str | None, str | None]]]:
    """Return each object id with its line numbers, and each object's isa and fileRef."""
    keys: dict[str, list[int]] = {}
    entries: list[tuple[str, int, str | None, str | None]] = []
    depth = 0
    inside = False
    open_depth = 0
    current: tuple[str, int, str | None, str | None] | None = None
    for number, line in enumerate(text.splitlines(), 1):
        if not inside:
            if OBJECTS.match(line):
                inside = True
                open_depth = depth + 1
            depth += depth_change(line)
            continue
        if current is None:
            match = ENTRY.match(line) if depth == open_depth else None
            if match:
                object_id = match.group(1) or match.group(2)
                current = (object_id, number, None, None)
                keys.setdefault(object_id, []).append(number)
        else:
            match = ISA.match(line)
            if match:
                current = (current[0], current[1], match.group(1), current[3])
            match = FILE_REF.match(line)
            if match:
                current = (current[0], current[1], current[2], match.group(1))
        depth += depth_change(line)
        if current is not None and depth == open_depth:
            entries.append(current)
            current = None
        if depth < open_depth:
            break
    return keys, entries


def project_problems(name: str, text: str) -> list[str]:
    """The duplicate ids and dangling build file references of one project."""
    keys, entries = scan(text)
    found = [
        f"{name}: {object_id} is defined {len(lines)} times, on lines "
        f"{', '.join(str(line) for line in lines)}"
        for object_id, lines in sorted(keys.items())
        if len(lines) > 1
    ]
    for object_id, line, isa, file_ref in entries:
        if isa == "PBXBuildFile" and file_ref is not None and file_ref not in keys:
            found.append(
                f"{name}:{line}: {object_id} builds {file_ref}, which is not an object"
            )
    return found


def problems(root: Path) -> list[str]:
    """Every problem of the Xcode projects of the repository at `root`."""
    return [
        problem
        for path in sorted(root.glob(PROJECTS))
        for problem in project_problems(
            path.relative_to(root).as_posix(), path.read_text(encoding="utf-8")
        )
    ]


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    found = problems(root)
    for problem in found:
        print(problem, file=sys.stderr)
    if found:
        return 1
    print("ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
