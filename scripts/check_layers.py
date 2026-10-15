#!/usr/bin/env python3
"""Fail when a Rust crate depends on a layer above it (AGENTS.md, "Architecture layers").

`LAYERS` lists, for every Soyeht crate, the Soyeht crates it may depend on. The check reads
every `Cargo.toml` of `engine/` and `apps/desktop/` and compares the dependency tables
(`dependencies`, `dev-dependencies`, `build-dependencies`, and the same tables under
`target.*`) with it. Crates outside the Soyeht workspace are not limited. A crate that is not
in the table fails too: adding a crate means placing it in a layer here and in AGENTS.md.
"""

from __future__ import annotations

import argparse
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST_ROOTS = ("engine", "apps/desktop")
TABLES = ("dependencies", "dev-dependencies", "build-dependencies")

FOUNDATION = {"soyeht-protocol", "soyeht-platform", "soyeht-process"}
# Layer 3: engine building blocks. They depend only on the foundation, and never on each other.
BLOCKS = (
    "soyeht-ptyd",
    "soyeht-acp",
    "soyeht-store",
    "soyeht-editor",
    "soyeht-writing",
    "soyeht-web",
    "soyeht-pane-state",
    "soyeht-usage",
    "soyeht-lsp",
    "soyeht-skills",
)
LAYERS: dict[str, set[str]] = {
    # Layer 2: wire models and rules, and the operating-system seams.
    "soyeht-protocol": set(),
    "soyeht-platform": set(),
    "soyeht-process": {"soyeht-platform"},
    **{block: set(FOUNDATION) for block in BLOCKS},
    "soyeht-vcs": {"soyeht-platform", "soyeht-process"},
    # Layer 4: the engine assembles the blocks; the two binaries pick what they need.
    "soyeht-engine": FOUNDATION | set(BLOCKS) | {"soyeht-vcs"},
    "soyeht-cli": {"soyeht-protocol", "soyeht-platform", "soyeht-ptyd", "soyeht-store"},
    "soyeht-mcp": {"soyeht-protocol", "soyeht-platform", "soyeht-skills"},
    # Layer 7: the Rust desktop client, its own workspace.
    "soyeht-client": {"soyeht-protocol", "soyeht-platform"},
    "soyeht-desktop": {"soyeht-client", "soyeht-protocol", "soyeht-platform"},
}


def dependency_tables(manifest: dict) -> list[dict]:
    tables = [manifest.get(name, {}) for name in TABLES]
    for target in manifest.get("target", {}).values():
        tables += [target.get(name, {}) for name in TABLES]
    return tables


def soyeht_dependencies(manifest: dict) -> set[str]:
    """Return the Soyeht crates that a manifest depends on, by their package names."""
    found: set[str] = set()
    for table in dependency_tables(manifest):
        for key, spec in table.items():
            package = spec.get("package", key) if isinstance(spec, dict) else key
            if package.startswith("soyeht-"):
                found.add(package)
    return found


def manifest_problems(relative: str, manifest: dict) -> list[str]:
    name = manifest.get("package", {}).get("name")
    if name is None:
        return []  # A pure workspace manifest declares no crate of its own.
    if name not in LAYERS:
        return [f"{relative}: crate {name} is in no layer; add it to LAYERS and to AGENTS.md"]
    return [
        f"{relative}: {name} depends on {dependency}, which is not in its layer "
        f"or below it (allowed: {', '.join(sorted(LAYERS[name])) or 'none'})"
        for dependency in sorted(soyeht_dependencies(manifest) - LAYERS[name])
    ]


def problems(root: Path) -> list[str]:
    found: list[str] = []
    seen: set[str] = set()
    for base in MANIFEST_ROOTS:
        for path in sorted((root / base).rglob("Cargo.toml")):
            relative = path.relative_to(root)
            if "target" in relative.parts:
                continue
            manifest = tomllib.loads(path.read_text(encoding="utf-8"))
            if "name" in manifest.get("package", {}):
                seen.add(manifest["package"]["name"])
            found += manifest_problems(relative.as_posix(), manifest)
    found += [f"LAYERS lists {name}, which has no Cargo.toml; remove it" for name in sorted(set(LAYERS) - seen)]
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    found = problems(parser.parse_args().root.resolve())
    for line in found:
        print(line)
    if found:
        print(f"{len(found)} layer violations")
    return 1 if found else 0


if __name__ == "__main__":
    raise SystemExit(main())
