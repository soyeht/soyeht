#!/usr/bin/env python3
"""Fail when a Rust crate depends on a layer above it (AGENTS.md, "Architecture layers").

`LAYERS` lists, for every Soyeht crate, the Soyeht crates it may depend on. The check reads
every `Cargo.toml` of `engine/`, `apps/desktop/` and `apps/android/` and compares the dependency tables
(`dependencies`, `dev-dependencies`, `build-dependencies`, and the same tables under
`target.*`) with it. Crates outside the Soyeht workspace are not limited. A crate that is not
in the table fails too: adding a crate means placing it in a layer here and in AGENTS.md.

The three cargo workspaces (`engine`, `apps/desktop`, `apps/android`) repeat a few settings that
must not drift: the `vt100` fork under `[patch.crates-io]` (ADR 0144), the `rust-version` and the
`[workspace.lints]` table. `workspace_problems` fails when a workspace differs from the engine's.
"""

from __future__ import annotations

import argparse
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST_ROOTS = ("engine", "apps/desktop", "apps/android")
TABLES = ("dependencies", "dev-dependencies", "build-dependencies")
# The first workspace is the reference; the others must match it on the settings below.
WORKSPACE_ROOTS = MANIFEST_ROOTS
PATCHED_CRATE = "vt100"

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
    "soyeht-ink",
)
LAYERS: dict[str, set[str]] = {
    # Layer 2: wire models and rules, and the operating-system seams.
    "soyeht-protocol": set(),
    "soyeht-platform": set(),
    "soyeht-process": {"soyeht-platform"},
    **{block: set(FOUNDATION) for block in BLOCKS},
    "soyeht-vcs": {"soyeht-platform", "soyeht-process"},
    # The service seam (ADR 0114): registers the engine and the supervisor with the system's
    # service manager. Only the command line uses it; the engine does not register itself.
    "soyeht-service": set(FOUNDATION),
    # Layer 4: the engine assembles the blocks; the two binaries pick what they need.
    "soyeht-engine": FOUNDATION | set(BLOCKS) | {"soyeht-vcs"},
    "soyeht-cli": {
        "soyeht-protocol",
        "soyeht-platform",
        "soyeht-ptyd",
        "soyeht-service",
        "soyeht-store",
    },
    "soyeht-mcp": {"soyeht-protocol", "soyeht-platform", "soyeht-skills"},
    # Layer 7: the Rust desktop client, its own workspace.
    "soyeht-client": {"soyeht-protocol", "soyeht-platform", "soyeht-process"},
    # The system web view of the desktop client (ADR 0110): it knows pages and pixels, and no
    # other crate of Soyeht.
    "soyeht-webview": set(),
    # Soyeht's Apache-2.0 stand-in for Zed's GPL-3.0 `ztracing` (ADR 0149): a proc-macro that does
    # nothing, patched in for the crate that GPUI's `sum_tree` uses. It depends on nothing.
    "ztracing": set(),
    "soyeht-desktop": {"soyeht-client", "soyeht-webview", "soyeht-protocol", "soyeht-platform"},
    # Layer 8: the Android app's core (ADR 0134), its own workspace. It reuses the desktop client's
    # connection and replica and holds no rule of its own.
    "soyeht-android": {"soyeht-client", "soyeht-protocol", "soyeht-platform"},
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
        # A crate may list itself as a development dependency to turn on its own features
        # for its tests (the `test-hooks` of ADR 0091); that is not a dependency on a layer.
        for dependency in sorted(soyeht_dependencies(manifest) - LAYERS[name] - {name})
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


def shared_settings(manifest: dict) -> dict[str, object]:
    """Return the settings every cargo workspace repeats, by a name for the message."""
    workspace = manifest.get("workspace", {})
    return {
        f"[patch.crates-io] {PATCHED_CRATE}": manifest.get("patch", {})
        .get("crates-io", {})
        .get(PATCHED_CRATE),
        "[workspace.package] rust-version": workspace.get("package", {}).get("rust-version"),
        "[workspace.lints]": workspace.get("lints"),
    }


def workspace_problems(root: Path) -> list[str]:
    """Return the settings in which a workspace differs from the first one (the engine's)."""
    settings: dict[str, dict[str, object]] = {}
    found: list[str] = []
    for base in WORKSPACE_ROOTS:
        path = root / base / "Cargo.toml"
        if not path.is_file():
            found.append(f"{base}/Cargo.toml: no workspace manifest to compare")
            continue
        settings[base] = shared_settings(tomllib.loads(path.read_text(encoding="utf-8")))
    reference = settings.get(WORKSPACE_ROOTS[0])
    if reference is None:
        return found
    for base, values in settings.items():
        for name, value in values.items():
            if value is None:
                found.append(f"{base}/Cargo.toml: {name} is missing")
            elif value != reference[name]:
                found.append(
                    f"{base}/Cargo.toml: {name} is {value!r} but {WORKSPACE_ROOTS[0]} has "
                    f"{reference[name]!r}; the three workspaces must match"
                )
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    root = parser.parse_args().root.resolve()
    found = problems(root) + workspace_problems(root)
    for line in found:
        print(line)
    if found:
        print(f"{len(found)} layer or workspace violations")
    return 1 if found else 0


if __name__ == "__main__":
    raise SystemExit(main())
