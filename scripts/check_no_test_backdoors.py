"""Fail when engine production code can change its behaviour for a test.

A debug build is what a developer installs, so a branch on `debug_assertions`, a marker file
named `test-...` or an environment variable named `SOYEHT_TEST_...` in an engine crate's
`src/` is a back door in the program people run (ADR 0091). The engine takes its test
behaviour through the `Hooks` port instead; the hooks that act are compiled only with the
`test-hooks` feature, which no default build enables.

The script reports, outside `ALLOWED`:

- `debug_assertions` in code (an attribute, `cfg!`, or any other use);
- a string literal that starts with `test-`, `test_` or `.test-`, or contains `SOYEHT_TEST_`;
- a read of the environment (`env::var`, `var_os`, `vars`, `vars_os`) whose name is built at
  run time, so no literal shows it: the first argument must be a string literal or a
  constant. The few sites that pass a name taken from a list are in `ENV_NAMES_FROM_LISTS`,
  each with its reason.

A cargo feature name (`feature = "test-support"`) is not a marker. Test code is exempt:
`#[cfg(test)]` items and the files compiled only for tests. It also fails when a crate makes
`test-hooks` a default feature or a normal or build dependency enables it, when an allowlist
entry no longer matches anything, and when an allowlisted module is not declared under
`#[cfg(feature = "test-hooks")]` in its parent module.
"""

from __future__ import annotations

import argparse
import re
import tomllib
from pathlib import Path

from hardening_gate import (
    is_rust_production_source,
    mask,
    test_module_files,
    without_cfg_test,
)

ROOT = Path(__file__).resolve().parents[1]
ENGINE = "engine"
FEATURE = "test-hooks"
# Modules that hold test hooks. Each is declared under `#[cfg(feature = "test-hooks")]` in its
# parent module, which the check enforces, so none of them exists in a product build.
ALLOWED = {
    "engine/soyeht-engine/src/test_hooks.rs": (
        "the hooks of soyeht-engine-test: the marker files and variables it reads"
    ),
    "engine/soyeht-engine/src/usage/fixture_host.rs": (
        "the fixture usage host of the end-to-end suite, reached only through the hooks"
    ),
}
# Reads of the environment whose name is not a literal, keyed `path::function(argument)`. Each
# passes a name taken from a fixed list of the program's own, never one a test can introduce.
ENV_NAMES_FROM_LISTS = {
    "engine/soyeht-engine/src/usage/host.rs::var(name)": (
        "the Host port's env lookup: the usage sources ask for the provider variables they document"
    ),
    "engine/soyeht-engine/src/usage/terminal.rs::var_os(name)": (
        "copies the INHERITED list (and on Windows the system list) into the child's environment"
    ),
    "engine/soyeht-ptyd/src/process.rs::var_os(name)": (
        "copies the inherited-variable list into the pane's environment"
    ),
    "engine/soyeht-platform/src/process/windows.rs::vars_os()": (
        "copies the whole environment into a launched child when the caller asks it to inherit"
    ),
}
DEBUG_ASSERTIONS = re.compile(r"\bdebug_assertions\b")
MARKER = re.compile(
    r'"(?:(?:test[-_]|\.test-)[^"\n]*|[^"\n]*SOYEHT_TEST_[^"\n]*)"'
)
FEATURE_NAME = re.compile(r'feature\s*=\s*$')
ENV_READ = re.compile(r"\benv::(var_os|vars_os|var|vars)\s*\(")
PLAIN_ARGUMENT = re.compile(r'"[^"\n]*"|(?:\w+::)*[A-Z][A-Z0-9_]*')
DEPENDENCY_TABLES = ("dependencies", "build-dependencies")


def call_argument(text: str, start: int) -> str:
    """The text of the first argument of a call whose `(` precedes `start`."""
    depth, end = 0, start
    while end < len(text):
        character = text[end]
        if character in "([{":
            depth += 1
        elif character in ")]}":
            if depth == 0:
                break
            depth -= 1
        elif character == "," and depth == 0:
            break
        end += 1
    return text[start:end].strip()


def production_masks(text: str) -> tuple[str, str]:
    """The text with comments and literals blanked, and with only literals kept, restricted to
    production code: every `#[cfg(test)]` item is blanked in both.

    A kept literal's characters are the original ones; a blanked one is example."""
    code = mask(text)[0]
    kept = mask(text, keep_strings=True)[0]
    # A literal's characters become `x` in the proxy, so a `#[cfg(test)]` item holding one is
    # blanked like any other code and a surviving `x` shows the literal is production.
    proxy = "".join(
        "x" if code[i] == " " and kept[i] not in " \n" else code[i] for i in range(len(kept))
    )
    return without_cfg_test(proxy), kept


def source_problems(
    path: str, text: str, excluded: set[str], used: set[str] | None = None
) -> list[tuple[int, str]]:
    """(line, message) for each back door in one file of an engine crate.

    `used` collects the `ENV_NAMES_FROM_LISTS` keys that matched, to find stale entries."""
    if path in excluded or path in ALLOWED:
        return []
    production, kept = production_masks(text)
    found: list[tuple[int, str]] = []
    for match in DEBUG_ASSERTIONS.finditer(production):
        found.append((match.start(), "branches on debug_assertions"))
    for match in MARKER.finditer(kept):
        if production[match.start()] != "x":
            continue
        if FEATURE_NAME.search(kept[: match.start()]):
            continue
        found.append((match.start(), f"test marker {match.group()}"))
    for match in ENV_READ.finditer(production):
        argument = call_argument(kept, match.end())
        if PLAIN_ARGUMENT.fullmatch(argument):
            continue
        key = f"{path}::{match.group(1)}({argument})"
        if key in ENV_NAMES_FROM_LISTS:
            if used is not None:
                used.add(key)
            continue
        found.append((match.start(), f"reads the environment by a name built at run time: {key}"))
    return [(text.count("\n", 0, offset) + 1, message) for offset, message in sorted(found)]


def engine_sources(root: Path) -> dict[str, str]:
    found: dict[str, str] = {}
    for path in sorted((root / ENGINE).rglob("*.rs")):
        relative = path.relative_to(root).as_posix()
        if "target" not in path.relative_to(root).parts and is_rust_production_source(relative):
            found[relative] = path.read_text(encoding="utf-8")
    return found


def manifest_problems(path: str, manifest: dict) -> list[str]:
    """The ways a crate could ship `test-hooks`: as a default feature or a normal dependency's."""
    problems = []
    if FEATURE in manifest.get("features", {}).get("default", []):
        problems.append(f"{path}: `{FEATURE}` is a default feature")
    tables = [manifest] + list(manifest.get("target", {}).values())
    for table in tables:
        for kind in DEPENDENCY_TABLES:
            for name, spec in table.get(kind, {}).items():
                if isinstance(spec, dict) and FEATURE in spec.get("features", []):
                    problems.append(f"{path}: {kind[:-1].replace('-', ' ')} `{name}` enables `{FEATURE}`")
    return problems


def parent_module(root: Path, relative: str) -> Path | None:
    """The file that declares the module in `relative`, when it exists."""
    path = root / relative
    candidates = (
        [path.parent / "lib.rs", path.parent / "main.rs"]
        if path.parent.name == "src"
        else [path.parent / "mod.rs", path.parent.with_suffix(".rs")]
    )
    return next((candidate for candidate in candidates if candidate.is_file()), None)


def allowlist_problems(root: Path) -> list[str]:
    """Allowlisted files that are gone, or that a product build would still compile."""
    found = []
    for relative in ALLOWED:
        if not (root / relative).is_file():
            found.append(f"{relative}: allowlisted but missing; delete its entry")
            continue
        parent = parent_module(root, relative)
        name = Path(relative).stem
        declaration = re.compile(
            rf'#\[cfg\(feature\s*=\s*"{FEATURE}"\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+{name}\b'
        )
        if parent is None or not declaration.search(parent.read_text(encoding="utf-8")):
            found.append(
                f'{relative}: not declared under #[cfg(feature = "{FEATURE}")] in its parent module'
            )
    return found


def problems(root: Path) -> list[str]:
    files = engine_sources(root)
    excluded = test_module_files({path: mask(text)[0] for path, text in files.items()})
    used: set[str] = set()
    found = [
        f"{path}:{line}: {message}"
        for path, text in files.items()
        for line, message in source_problems(path, text, excluded, used)
    ]
    found.extend(
        f"{key}: allowlisted environment read that no longer exists; delete its entry"
        for key in ENV_NAMES_FROM_LISTS
        if key not in used
    )
    found.extend(allowlist_problems(root))
    for manifest_path in sorted((root / ENGINE).glob("*/Cargo.toml")):
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        found.extend(manifest_problems(manifest_path.relative_to(root).as_posix(), manifest))
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    found = problems(parser.parse_args().root.resolve())
    for line in found:
        print(line)
    if found:
        print(f"{len(found)} test back doors in engine production code (ADR 0091)")
    return 1 if found else 0


if __name__ == "__main__":
    raise SystemExit(main())
