#!/usr/bin/env python3
"""Fail when production code types a wire method, event or setting name by hand.

`contracts/protocol.json` defines every method and event name once. The generated forms are
`RequestMethod` and `EventName` in Swift, and `method::*` and `event::*` string constants
(also usable in `match` patterns) and `EventName` in Rust. A string literal equal to one of
those names anywhere in production code is a copy that can drift: use the generated form.

The same holds for the setting keys of `x-settings`, whose generated forms are `Setting` in
Swift and `setting::*` in Rust, and for the families of keys that end in `.<name>` (a literal
that starts with the family's prefix, such as `"harness.defaultMode."`, is a copy too). A file
whose first line says `Generated from contracts/` is an output and is not scanned.

Test code is exempt: Swift `Tests` directories, Rust `tests/` directories, `#[cfg(test)]`
items and files compiled only for tests, and the migration files of `FROZEN_NAMES`. The name `hello` has no dot and is too common as
plain text, so it is not checked. Run the script to list every offending literal.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from check_swallowed_errors import IGNORED_FILES, IGNORED_PARTS
from hardening_gate import (
    is_rust_production_source,
    mask,
    test_module_files,
    without_cfg_test,
)

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = "contracts/protocol.json"
SWIFT_ROOTS = (
    "apps/shared/Sources",
    "apps/dictation/Sources",
    "apps/mac/Sources",
    "apps/ios/Sources",
)
RUST_ROOTS = ("engine", "apps/desktop")
# Files that must keep the names as they were written: a migration rewrites stored history, so
# renaming an event in the contract must not change what it matches.
FROZEN_NAMES = {
    "engine/soyeht-store/src/english_values.rs": "rewrites events stored before the English rename",
}
GENERATED = ("// Generated from contracts/", "# Generated from contracts/")
LITERAL = re.compile(r'"([a-z][A-Za-z0-9_]*(?:\.[A-Za-z0-9_{}]*)+)"')


def wire_names(root: Path) -> tuple[dict[str, str], list[str]]:
    """Map every dotted method, event and setting name to its kind, and list the key families.

    The kinds are `method`, `event` and `setting`. A family (`harness.defaultMode.<harnessId>`)
    is given by its prefix, `harness.defaultMode.`.
    """
    schema = json.loads((root / SCHEMA).read_text(encoding="utf-8"))
    names = {name: "method" for name in schema["x-methods"]}
    names.update({name: "event" for name in schema["x-events"]})
    settings = schema.get("x-settings", {})
    names.update({key: "setting" for key in settings if "<" not in key})
    prefixes = [key[: key.index("<")] for key in settings if "<" in key]
    return {name: kind for name, kind in names.items() if "." in name}, prefixes


def production_literals(path: str, text: str, excluded: set[str]) -> list[tuple[int, str]]:
    """Return (offset, text) of every string literal that sits in production code."""
    if path in excluded:
        return []
    kept = mask(text, keep_strings=True)[0]
    blank = mask(text)[0]
    # A literal's characters become `x`, so that a `#[cfg(test)]` item that holds one is
    # blanked like any other code, and a surviving `x` shows the literal is production.
    proxy = "".join(
        "x" if blank[i] == " " and kept[i] not in " \n" else blank[i] for i in range(len(kept))
    )
    production = without_cfg_test(proxy) if path.endswith(".rs") else proxy
    return [
        (found.start(), found.group(1))
        for found in LITERAL.finditer(kept)
        if production[found.start()] == "x"
    ]


def sources(root: Path) -> dict[str, str]:
    found: dict[str, str] = {}
    for relative in SWIFT_ROOTS:
        for path in (root / relative).rglob("*.swift"):
            parts = path.relative_to(root).parts
            if path.name not in IGNORED_FILES and not IGNORED_PARTS.intersection(parts):
                found[path.relative_to(root).as_posix()] = path.read_text(encoding="utf-8")
    for relative in RUST_ROOTS:
        for path in (root / relative).rglob("*.rs"):
            name = path.relative_to(root).as_posix()
            if not IGNORED_PARTS.intersection(path.relative_to(root).parts):
                found[name] = path.read_text(encoding="utf-8")
    return found


def sites(root: Path) -> list[tuple[str, int, str, str]]:
    """Return (path, line, name, kind) for every hand-typed wire name."""
    names, prefixes = wire_names(root)
    files = sources(root)
    rust_masks = {
        path: mask(text)[0] for path, text in files.items() if is_rust_production_source(path)
    }
    excluded = test_module_files(rust_masks)
    found: list[tuple[str, int, str, str]] = []
    for path, text in sorted(files.items()):
        if path in FROZEN_NAMES or (path.endswith(".rs") and path not in rust_masks):
            continue
        if text.startswith(GENERATED):
            continue
        for offset, name in production_literals(path, text, excluded):
            kind = names.get(name) or (
                "setting" if any(name.startswith(prefix) for prefix in prefixes) else None
            )
            if kind:
                found.append((path, text.count("\n", 0, offset) + 1, name, kind))
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    found = sites(parser.parse_args().root.resolve())
    for path, line, name, kind in found:
        print(f"{path}:{line}: hand-typed {kind} name {name!r}; use the generated form")
    if found:
        print(f"{len(found)} hand-typed wire names")
    return 1 if found else 0


if __name__ == "__main__":
    raise SystemExit(main())
