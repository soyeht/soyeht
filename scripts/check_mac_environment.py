#!/usr/bin/env python3
"""Keep the Mac app's observable environment objects injected at the root.

A view that reads `@Environment(Model.self)` crashes (EXC_BREAKPOINT in the `EnvironmentValues`
getter) when no ancestor injected a `Model`. An overlay or sheet added after an
`.environment(model)` modifier is not under it, so an injection made inside a view is easy to
get wrong (the Preferences panel lost `WebLinkPreferences` this way). The rule: every object
environment is created and injected once, in `SoyehtMacApp.swift`, and every
`@Environment(Model.self)` read names a model injected there. Key-path values
(`.environment(\\.key, value)`) have defaults and are not covered.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT_FILE = "apps/mac/Sources/SoyehtMacApp.swift"
SOURCES = "apps/mac/Sources"

READ = re.compile(r"@Environment\(\s*([A-Z]\w*)\.self\s*\)")
INJECT = re.compile(r"\.environment\(\s*([a-z]\w*)\s*\)")
DECLARATION = re.compile(r"@State\s+(?:private\s+)?var\s+(\w+)\s*=\s*([A-Z]\w*)\b")


def injected_types(root_source: str) -> set[str]:
    """The model types the root creates and passes to `.environment(_:)`."""
    declared = {name: kind for name, kind in DECLARATION.findall(root_source)}
    return {declared[name] for name in INJECT.findall(root_source) if name in declared}


def reads(source: str) -> list[tuple[int, str]]:
    return [
        (source.count("\n", 0, match.start()) + 1, match.group(1))
        for match in READ.finditer(source)
    ]


def injections(source: str) -> list[int]:
    return [source.count("\n", 0, match.start()) + 1 for match in INJECT.finditer(source)]


def violations(files: dict[str, str], root: str = ROOT_FILE) -> list[str]:
    provided = injected_types(files.get(root, ""))
    found: list[str] = []
    for path, source in sorted(files.items()):
        for line, kind in reads(source):
            if kind not in provided:
                found.append(f"{path}:{line}: @Environment({kind}.self) is not injected in {root}")
        if path != root:
            for line in injections(source):
                found.append(f"{path}:{line}: inject object environments in {root}, not here")
    return found


def main() -> int:
    repo = pathlib.Path(__file__).resolve().parent.parent
    files = {
        str(path.relative_to(repo)): path.read_text(encoding="utf-8")
        for path in sorted((repo / SOURCES).rglob("*.swift"))
    }
    problems = violations(files)
    for problem in problems:
        print(problem, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
