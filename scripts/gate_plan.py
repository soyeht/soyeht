#!/usr/bin/env python3
"""Map the paths a change touches to the gate areas the fast tier runs (ADR 0089).

`scripts/check_all.sh` is the only list of gate steps and tags each with an area. This
script decides which areas a change needs, which engine crates `cargo test` covers, and
whether the change needs a reviewer. The fast tier is cheap on purpose: format, lint, a build
and the unit tests (`--lib --bins`) of the selected areas. Integration tests, the Swift
tests, the Mac and iOS app builds, e2e, the editor measurement and the timing tests run only
in the full tier, after the merge (`FULL_ONLY_AREAS`). Rules, first match wins:

- a change to the contracts, `engine/soyeht-protocol`, the gate tooling (`scripts/`,
  `.github/`, `.githooks/`), the lockfiles or the workspace manifests selects every
  code area, and so does a cargo workspace that cannot be read;
- a file that another directory reads (`CROSS_READS`) also selects the readers;
- a path under one engine crate selects `engine` and that crate plus its reverse
  dependencies in the engine workspace (`cargo metadata`); a crate that the desktop
  workspace depends on also selects `desktop`;
- the other code directories select their own area;
- documentation selects only `always`;
- an unknown path selects every code area.

A change to the scripts that run the gate (`gate_runner`) upgrades the run to the full
tier: a mistake there is not covered by a partial run. A change to the unit tests of those
scripts alone (`scripts/test_*.py`) does not: it selects `always`, whose steps run those
tests.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import posixpath
import re
import shlex
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from gate_base import BaseError, main_health_dir, resolve_base

ROOT = Path(__file__).resolve().parents[1]
CODE_AREAS = ("engine", "desktop", "swift", "typescript")
# Areas that only the full tier (the run after the merge) covers: the dictation tests, the
# Mac and iOS app builds, the end-to-end suites, the editor measurement and the timing tests.
# The Swift packages and the engine and desktop workspaces also have steps that are full-only.
FULL_ONLY_AREAS = ("dictation", "mac_app", "ios_app", "e2e", "editor_perf", "timing")
ALL_AREAS = ("always", *CODE_AREAS, *FULL_ONLY_AREAS)

# Data files of the ratchets: the always-on checks read them, no code depends on them.
GATE_DATA_PATTERNS = ("scripts/*baseline*.json", "scripts/*allowlist*.json")
# Ratchet data that records a reason or an exception, so a change to it needs a reviewer.
RATCHET_EXCEPTIONS = ("scripts/quality_baseline_justifications.json", "scripts/dead_pub_allowlist.json")
CONTRACT_SOURCES = {"contracts/protocol.json", "contracts/profiles.json", "contracts/design-tokens.json"}
WORKSPACE_FILES = {
    "engine/Cargo.toml", "engine/Cargo.lock", "apps/desktop/Cargo.toml",
    "apps/desktop/Cargo.lock", "rust-toolchain.toml",
}
DOC_ROOT_FILES = {"AGENTS.md", "CLAUDE.md", "README.md", "LICENSE"}
# Markdown that the engine compiles in (`include_str!`), so it is code.
ENGINE_PROMPTS = "docs/contracts/prompts/"

# Files that another directory reads at build or test time, which neither the directory
# layout nor Cargo shows: (path prefix, areas it also selects, engine crates it also tests).
# test_gate_plan scans the engine and desktop sources and fails on a read that is not here.
CROSS_READS = (
    ("apps/mac/SoyehtMac.xcodeproj/", ("swift", "engine"), ("soyeht-engine",)),
    ("apps/mac/Resources/Fonts/", ("swift", "desktop"), ()),
    ("apps/shared/Sources/SoyehtClient/Resources/", ("swift", "desktop"), ()),
)

ENGINE_CORE = ("core", "dispatch", "server", "agents")
SECURITY_ENGINE = ("auth", "pairing", "push", "network", "ui_attestation")


@dataclass(frozen=True)
class Package:
    name: str
    directory: str  # relative to the repository root, POSIX
    dependencies: tuple[str, ...]


@dataclass(frozen=True)
class Plan:
    base: str | None
    files: tuple[str, ...]
    areas: tuple[str, ...]
    # None runs the whole engine workspace.
    engine_crates: tuple[str, ...] | None
    review: str
    review_reasons: tuple[str, ...]
    # Why the run must be the full tier; empty when the fast tier is enough.
    upgrade: tuple[str, ...] = ()


def gate_data(path: str) -> bool:
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in GATE_DATA_PATTERNS)


def gate_test(path: str) -> bool:
    """A unit test of the gate scripts: the `always` steps run it, and nothing else reads it."""
    return posixpath.dirname(path) == "scripts" and re.fullmatch(r"test_\w+\.py", posixpath.basename(path)) is not None


def gate_runner(path: str) -> bool:
    """The scripts that run or define the gate; a partial run cannot vouch for a change here."""
    name = posixpath.basename(path)
    return path.startswith(".github/") or (
        path.startswith("scripts/")
        and not gate_test(path)
        and (name == "check_all.sh"
             or name.startswith(("gate_", "post_merge", "main_watch", "remote_gate",
                                 "e2e", "tcc_guard", "require_one_rust_test"))
             or name in {"check_mac.sh", "check_ios.sh", "check_swift.sh", "check_editor_performance.py"})
    )


def gate_tooling(path: str) -> bool:
    if gate_data(path):
        return False
    return path.startswith(("scripts/", ".github/", ".githooks/"))


def forces_everything(path: str) -> bool:
    return (
        path in CONTRACT_SOURCES
        or path in WORKSPACE_FILES
        or path.startswith("engine/soyeht-protocol/")
        or (gate_tooling(path) and not gate_test(path))
        or (path.startswith("contracts/")
            and not path.startswith(("contracts/swift/", "contracts/typescript/"))
            and not path.endswith(".md"))
    )


def is_documentation(path: str) -> bool:
    if path.startswith(ENGINE_PROMPTS):
        return False
    if path in DOC_ROOT_FILES or gate_data(path) or path.startswith(("docs/", "design/")):
        return True
    # Markdown elsewhere may be compiled in or read by a test: only READMEs are documents.
    return path.endswith(".md") and posixpath.basename(path) == "README.md"


def package_of(path: str, packages: list[Package]) -> Package | None:
    best = None
    for package in packages:
        if path.startswith(package.directory + "/") and (
            best is None or len(package.directory) > len(best.directory)
        ):
            best = package
    return best


def reverse_closure(names: set[str], packages: list[Package]) -> set[str]:
    """`names` and the crates that depend on them."""
    closure = set(names)
    grew = True
    while grew:
        grew = False
        for package in packages:
            if package.name not in closure and closure.intersection(package.dependencies):
                closure.add(package.name)
                grew = True
    return closure


def desktop_editor(path: str) -> bool:
    if not path.startswith("apps/desktop/") or "/src/" not in path:
        return False
    return any(part.startswith("editor") for part in path.split("/src/", 1)[1].split("/"))


def review_reasons(path: str) -> list[str]:
    engine = "engine/soyeht-engine/src/"
    name = path[len(engine):].split("/")[0].split(".")[0] if path.startswith(engine) else ""
    manifest = posixpath.basename(path) in {"Cargo.toml", "Cargo.lock", "Package.swift", "Package.resolved"}
    checks = (
        ("engine core", name.startswith(ENGINE_CORE) or path.startswith("engine/soyeht-store/")),
        ("protocol", path.startswith("engine/soyeht-protocol/")
         or path in {"contracts/protocol.json", "contracts/profiles.json"}),
        ("editor", path.startswith(("apps/mac/Sources/Editor", "engine/soyeht-editor/", engine + "editor/"))
         or desktop_editor(path)),
        ("security", path.startswith(("engine/soyeht-platform/src/", "engine/soyeht-ptyd/"))
         or name.startswith(SECURITY_ENGINE)
         or ("permissions" in path and "web" in path)),
        ("dependencies", manifest),
        ("ratchet exception", path in RATCHET_EXCEPTIONS),
        ("gate scripts", gate_tooling(path)),
    )
    return [f"{label}: {path}" for label, hit in checks if hit]


def make_plan(
    files: list[str],
    engine_packages: list[Package] | None,
    desktop_engine_deps: set[str],
    base: str | None = None,
) -> Plan:
    """`engine_packages` is None when a cargo workspace cannot be read: fail closed."""
    areas = {"always"}
    crates: set[str] = set()
    everything = engine_packages is None
    for path in files:
        for prefix, extra_areas, extra_crates in CROSS_READS:
            if path.startswith(prefix):
                areas.update(extra_areas)
                crates.update(extra_crates)
        if forces_everything(path):
            everything = True
        elif is_documentation(path) or gate_test(path):
            continue
        elif path.startswith("engine/") or path.startswith(ENGINE_PROMPTS):
            areas.add("engine")
            package = package_of(path, engine_packages or [])
            if package is None:
                everything = True
            else:
                crates.add(package.name)
                if package.name in desktop_engine_deps:
                    areas.add("desktop")
        elif path.startswith("apps/desktop/"):
            areas.add("desktop")
        elif path.startswith(("apps/dictation/", "apps/shared/", "contracts/swift/", "apps/mac/", "apps/ios/")):
            areas.add("swift")
        elif path.startswith("contracts/typescript/"):
            areas.add("typescript")
        else:
            everything = True
    if everything:
        areas.update(CODE_AREAS)
    selection = None
    if "engine" in areas and not everything:
        selection = tuple(sorted(reverse_closure(crates, engine_packages or [])))
    reasons = sorted({reason for path in files for reason in review_reasons(path)})
    upgrade = tuple(sorted(path for path in files if gate_runner(path)))
    return Plan(
        base,
        tuple(files),
        tuple(area for area in ALL_AREAS if area in areas),
        selection,
        "required" if reasons else "optional",
        tuple(reasons),
        upgrade,
    )


def full_plan() -> Plan:
    return Plan(None, (), ALL_AREAS, None, "required", ("full tier",))


def metadata(manifest: Path) -> list[dict]:
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked",
         "--manifest-path", str(manifest)],
        capture_output=True, text=True, cwd=ROOT,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or "cargo metadata failed")
    return json.loads(result.stdout)["packages"]


def to_packages(raw: list[dict], root: Path = ROOT) -> list[Package]:
    members = {package["name"] for package in raw}
    packages = []
    for package in raw:
        directory = PurePosixPath(Path(package["manifest_path"]).parent.relative_to(root).as_posix())
        dependencies = tuple(d["name"] for d in package["dependencies"] if d["name"] in members)
        packages.append(Package(package["name"], str(directory), dependencies))
    return packages


def desktop_engine_dependencies(raw: list[dict], root: Path = ROOT) -> set[str]:
    """Engine crates that the desktop workspace depends on by path."""
    engine = (root / "engine").resolve()
    return {
        dependency["name"]
        for package in raw
        for dependency in package["dependencies"]
        if dependency.get("path") and Path(dependency["path"]).resolve().is_relative_to(engine)
    }


def changed_files(base: str) -> list[str]:
    """Files differing from `base` in the commits and the working tree, plus new ones."""
    def git(*arguments: str) -> list[str]:
        output = subprocess.check_output(["git", *arguments], cwd=ROOT, text=True)
        return [line for line in output.splitlines() if line]

    tracked = git("diff", "--name-only", "--no-renames", base)
    untracked = git("ls-files", "--others", "--exclude-standard")
    return sorted(set(tracked) | set(untracked))


def render_text(plan: Plan, tier: str) -> str:
    lines = [f"gate plan (tier {tier})"]
    if plan.base:
        lines.append(f"  base:    {plan.base}")
        lines.append(f"  changed: {len(plan.files)} files")
    lines.append(f"  areas:   {' '.join(plan.areas)}")
    if tier == "fast":
        tests = "whole workspace" if plan.engine_crates is None else " ".join(plan.engine_crates)
        if "engine" in plan.areas:
            lines.append(f"  engine tests: {tests}")
        lines.append(f"  after the merge only: {' '.join(FULL_ONLY_AREAS)}, integration and Swift tests")
    if plan.upgrade:
        lines.append(f"  full tier required: {', '.join(plan.upgrade)}")
    lines.append(f"  review:  {plan.review}")
    lines.extend(f"    {reason}" for reason in plan.review_reasons)
    return "\n".join(lines)


def render_shell(plan: Plan) -> str:
    tests = "--workspace" if plan.engine_crates is None else " ".join(f"-p {c}" for c in plan.engine_crates)
    values = {
        "GATE_AREAS": " ".join(plan.areas),
        "GATE_ENGINE_TEST_ARGS": tests,
        "GATE_REVIEW": plan.review,
        "GATE_UPGRADE": ", ".join(plan.upgrade),
    }
    return "\n".join(f"{key}={shlex.quote(value)}" for key, value in values.items())


def main_health_warning(health: Path) -> str | None:
    """A warning when the post-merge run says main is red."""
    try:
        latest = json.loads((health / "latest.json").read_text())
    except (OSError, ValueError):
        return None  # no post-merge run has been recorded here
    if latest.get("result") != "fail":
        return None
    culprit = (latest.get("culprit") or {}).get("sha", "unknown")
    return (
        f"WARNING: main is RED at {str(latest.get('sha'))[:12]} (step {latest.get('failed_step')}, "
        f"culprit {culprit[:12]}); fix or revert it first and merge nothing into the affected area"
    )


def build_plan(base_argument: str | None, files_from: str | None) -> Plan:
    base = resolve_base(ROOT, base_argument) if files_from is None else base_argument
    files = (
        changed_files(base) if files_from is None
        else [line.strip() for line in Path(files_from).read_text().splitlines() if line.strip()]
    )
    try:
        engine = metadata(ROOT / "engine/Cargo.toml")
        packages: list[Package] | None = to_packages(engine)
        desktop = desktop_engine_dependencies(metadata(ROOT / "apps/desktop/Cargo.toml"))
    except (RuntimeError, OSError, KeyError, ValueError) as error:
        print(f"gate_plan: cannot read the cargo workspaces ({error}); selecting every code area",
              file=sys.stderr)
        packages, desktop = None, set()
    return make_plan(files, packages, desktop, base)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tier", choices=("fast", "full"), default="fast")
    parser.add_argument("--base", help="full commit OID (default: merge-base of origin/main and HEAD)")
    parser.add_argument("--format", choices=("text", "shell", "json"), default="text")
    parser.add_argument("--files-from", help="read the changed paths from this file instead of git")
    arguments = parser.parse_args(argv)
    try:
        plan = full_plan() if arguments.tier == "full" else build_plan(arguments.base, arguments.files_from)
    except BaseError as error:
        print(f"gate_plan: {error}", file=sys.stderr)
        return 2
    if arguments.tier == "fast":
        try:
            warning = main_health_warning(main_health_dir(ROOT))
        except (subprocess.CalledProcessError, OSError):
            warning = None
        if warning:
            print(warning, file=sys.stderr)
    if arguments.format == "shell":
        # The human-readable plan goes to the gate log; stdout is for `eval`.
        print(render_text(plan, arguments.tier), file=sys.stderr)
        print(render_shell(plan))
    elif arguments.format == "json":
        print(json.dumps(plan.__dict__, indent=2))
    else:
        print(render_text(plan, arguments.tier))
    return 0


if __name__ == "__main__":
    sys.exit(main())
