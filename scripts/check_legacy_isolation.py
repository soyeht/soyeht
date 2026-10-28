#!/usr/bin/env python3
"""Fail when the rewrite could touch the owner's legacy app (AGENTS.md, "Legacy app").

Two rules, both from `legacy_guard.py`:

1. Every machine identifier of `contracts/profiles.json` (data directories, application ids,
   service labels) contains "next" and names nothing of the legacy app.
2. No tracked file names a legacy bundle identifier, `/Applications/Soyeht.app` or a legacy data
   folder, except the files in `ALLOWED`, which exist to say that these are forbidden.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

from legacy_guard import LEGACY_TEXT, MARKER, names_legacy

ROOT = Path(__file__).resolve().parents[1]
PROFILES = "contracts/profiles.json"
ALLOWED = {
    "AGENTS.md": "the rule that forbids them",
    "docs/adr/0112-production-profile-and-native-data-directories.md": "the decision that avoids them",
    "docs/contracts/00-slice-1-objective.md": "the first slice's list of names it must not use",
    "scripts/legacy_guard.py": "the guard's own list",
    "scripts/check_legacy_isolation.py": "this check",
    "contracts/test_generator.py": "tests that the generator refuses them",
    "scripts/test_install_dev.py": "tests that the installer refuses them",
    "scripts/test_check_legacy_isolation.py": "tests of this check",
}


def identifiers(document: dict) -> list[tuple[str, str]]:
    """Return (what, value) for every identifier the profiles define."""
    found = []
    for name, profile in document["profiles"].items():
        for system, path in profile["dataDirectory"].items():
            found.append((f"{name} data directory ({system})", path.split("/")[0]))
        for system, app_id in profile["appIds"].items():
            found.append((f"{name} app id ({system})", app_id))
        for component, label in profile["launchdLabels"].items():
            found.append((f"{name} {component} service label", label))
    return found


def profile_problems(document: dict) -> list[str]:
    return [
        f'{PROFILES}: {what} "{value}" must contain "{MARKER}" and name nothing of the legacy app'
        for what, value in identifiers(document)
        if MARKER not in value.lower() or names_legacy(value)
    ]


def file_problems(texts: dict[str, str]) -> list[str]:
    problems = []
    for path, text in sorted(texts.items()):
        if path in ALLOWED:
            continue
        for number, line in enumerate(text.splitlines(), 1):
            if match := LEGACY_TEXT.search(line):
                problems.append(
                    f"{path}:{number}: names the legacy app ({match.group(0)}); "
                    "the rewrite never reads or writes it"
                )
    return problems


def tracked_texts(root: Path) -> dict[str, str]:
    names = subprocess.run(
        ["git", "ls-files", "-z"], cwd=root, check=True, capture_output=True
    ).stdout.decode().split("\0")
    texts = {}
    for name in filter(None, names):
        path = root / name
        if not path.is_file():
            continue  # A path that is tracked but deleted in the working tree.
        try:
            texts[name] = path.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue  # Binary files hold no identifiers to check.
    return texts


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args(argv)
    document = json.loads((args.root / PROFILES).read_text(encoding="utf-8"))
    problems = profile_problems(document) + file_problems(tracked_texts(args.root))
    for problem in problems:
        print(problem, file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    raise SystemExit(main())
