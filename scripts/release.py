#!/usr/bin/env python3
"""Show, bump and check the product version (ADR 0115).

`contracts/release.json` is the one source. `contracts/generate.py` writes the Swift constant
from it; the files that cannot be generated (the two Cargo workspaces and their lockfiles, the
npm package, the Mac and iOS Xcode projects) are rewritten here and checked here:

    uv run python scripts/release.py show [version|revision|build-number]
    uv run python scripts/release.py bump major|minor|patch|revision|X.Y.Z [--dry-run]
    uv run python scripts/release.py sync [--dry-run]
    uv run python scripts/release.py check

This script never tags, pushes, builds or signs. Packaging and notarization are not here yet:
they need accounts and certificates that do not exist. When they do, each is one function that
reads `current(ROOT)` (the version, the revision and the build number) and one entry in
`COMMANDS`; they sign with credentials taken from the environment, never from this repository.
"""

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "contracts"))

import product_version  # noqa: E402  (the contracts directory is on the path just above)
from product_version import BUMPS, RELEASE_SOURCE, Release, Version  # noqa: E402

GENERATOR = "contracts/generate.py"
SHOW_FIELDS = ("version", "revision", "build-number")


@dataclass(frozen=True)
class Place:
    """One hand-kept value: where it is, how to find it and what it must say."""

    path: str
    what: str
    pattern: re.Pattern
    expected: Callable[[Release], str]
    # A place that may be absent but must agree when present (a plist the build settings fill).
    optional: bool = False

    def matches(self, text):
        return list(self.pattern.finditer(text))

    def label(self, match):
        """Where a match is, for a message; every pattern has the groups `pre`, `value` and `post`, and may have `name`."""
        name = match.groupdict().get("name")
        line = match.string.count("\n", 0, match.start("value")) + 1
        return f"{self.path}:{line} ({self.what}{', ' + name if name else ''})"

    def rewritten(self, text, made):
        wanted = self.expected(made)
        return self.pattern.sub(lambda m: f"{m['pre']}{wanted}{m['post']}", text)


def _version(made):
    return str(made.version)


def _build(made):
    return str(made.build_number())


def _pattern(text, flags=re.MULTILINE):
    return re.compile(text, flags)


def _cargo_workspace(path):
    pattern = _pattern(r'(?P<pre>\[workspace\.package\][^\[]*?^version\s*=\s*")(?P<value>[^"]*)(?P<post>")', re.M | re.S)
    return Place(path, "workspace package version", pattern, _version)


def _cargo_lock(path):
    # A package of the workspace has no `source` line; a crate from the registry does.
    pattern = _pattern(
        r'(?P<pre>\[\[package\]\]\nname = "(?P<name>[^"]+)"\nversion = ")(?P<value>[^"]*)(?P<post>"\n)(?!source = )'
    )
    return Place(path, "workspace crate", pattern, _version)


def _npm(path):
    return Place(path, "package version", _pattern(r'(?P<pre>^  "version":\s*")(?P<value>[^"]*)(?P<post>")'), _version)


def _npm_lock_root(path):
    # The entry for the package itself: the first of `packages`, before any dependency.
    pattern = _pattern(
        r'(?P<pre>"packages": \{\n    "": \{\n(?:      .*\n)*?      "version": ")(?P<value>[^"]*)(?P<post>")'
    )
    return Place(path, "root package entry", pattern, _version)


def _xcode(path, key, expected, what):
    return Place(path, what, _pattern(rf'(?P<pre>"{key}" = ")(?P<value>[^"]*)(?P<post>";)'), expected)


def _plist(path, key, variable):
    pattern = _pattern(rf"(?P<pre><key>{key}</key>\s*<string>)(?P<value>[^<]*)(?P<post></string>)")
    return Place(path, key, pattern, lambda _release: variable, optional=True)


XCODE_PROJECTS = ("apps/mac/SoyehtMac.xcodeproj/project.pbxproj", "apps/ios/SoyehtPhone.xcodeproj/project.pbxproj")
INFO_PLISTS = ("apps/mac/Resources/Info.plist", "apps/ios/Info.plist")

PLACES = (
    _cargo_workspace("engine/Cargo.toml"),
    _cargo_lock("engine/Cargo.lock"),
    _cargo_workspace("apps/desktop/Cargo.toml"),
    _cargo_lock("apps/desktop/Cargo.lock"),
    _npm("contracts/typescript/package.json"),
    _npm("contracts/typescript/package-lock.json"),
    _npm_lock_root("contracts/typescript/package-lock.json"),
    *(_xcode(path, "MARKETING_VERSION", _version, "MARKETING_VERSION") for path in XCODE_PROJECTS),
    *(_xcode(path, "CURRENT_PROJECT_VERSION", _build, "CURRENT_PROJECT_VERSION, the build number") for path in XCODE_PROJECTS),
    # The bundles take both numbers from the project settings above (the Mac one has none of
    # these keys: Xcode fills them in); a literal here would silently win over the settings.
    *(_plist(path, "CFBundleShortVersionString", "$(MARKETING_VERSION)") for path in INFO_PLISTS),
    *(_plist(path, "CFBundleVersion", "$(CURRENT_PROJECT_VERSION)") for path in INFO_PLISTS),
)


# The crates of both workspaces; each takes its version from `[workspace.package]`.
MEMBER_MANIFESTS = ("engine/*/Cargo.toml", "apps/desktop/crates/*/Cargo.toml")
INHERITS = re.compile(r"^version\.workspace\s*=\s*true\s*$", re.MULTILINE)


def current(root=ROOT):
    """The release in `contracts/release.json`: the product version and its build revision."""
    return product_version.load((root / RELEASE_SOURCE).read_text())


def problems(root=ROOT):
    """What disagrees with the source, one line each; empty when every place agrees."""
    try:
        made = current(root)
    except (OSError, ValueError) as failure:
        return [f"{RELEASE_SOURCE}: {failure}"]
    found = []
    for place in PLACES:
        try:
            text = (root / place.path).read_text()
        except OSError as failure:
            found.append(f"{place.path}: cannot be read ({failure.strerror})")
            continue
        matches = place.matches(text)
        if not matches and not place.optional:
            found.append(f"{place.path}: no {place.what} found; update the pattern in scripts/release.py")
        wanted = place.expected(made)
        found.extend(
            f"{place.label(match)} is {match['value']}, the source says {wanted}"
            for match in matches
            if match["value"] != wanted
        )
    for pattern in MEMBER_MANIFESTS:
        found.extend(
            f"{manifest.relative_to(root).as_posix()}: has no `version.workspace = true`, so it keeps a version of its own"
            for manifest in sorted(root.glob(pattern))
            if not INHERITS.search(manifest.read_text())
        )
    return found


def next_release(old, target):
    """The release a `bump` asks for.

    A part to raise, `revision` to upload the same version again, or an explicit version above
    `old`. Every new version starts again at revision 0.
    """
    if target in BUMPS:
        return old.bumped(target)
    new = product_version.parse(target)
    if new <= old.version:
        raise ValueError(f"{new} is not above {old.version}; a version only goes up (`bump revision` uploads it again)")
    return product_version.release(new)


def rewrites(root, made):
    """Every file the release touches, as {path: new text}; reads all before anything is written."""
    texts = {}
    for place in PLACES:
        text = texts.get(place.path)
        if text is None:
            try:
                text = (root / place.path).read_text()
            except OSError as failure:
                raise ValueError(f"{place.path} cannot be read ({failure.strerror})") from failure
        if not place.matches(text) and not place.optional:
            raise ValueError(f"{place.path}: no {place.what} found; update the pattern in scripts/release.py")
        texts[place.path] = place.rewritten(text, made)
    texts[RELEASE_SOURCE] = product_version.dump(made)
    return texts


def apply(root, made, dry_run=False):
    """Write `made` to the source and to every place; return the paths that changed."""
    changed = []
    for path, text in rewrites(root, made).items():
        if (root / path).read_text() != text:
            changed.append(path)
            if not dry_run:
                (root / path).write_text(text)
    return changed


def bump(root, target, dry_run=False):
    old = current(root)
    new = next_release(old, target)
    return old, new, apply(root, new, dry_run)


def run_generator(*arguments):
    return subprocess.run([sys.executable, str(ROOT / GENERATOR), *arguments], cwd=ROOT).returncode


def describe(made):
    return f"{made.version} revision {made.revision} (build number {made.build_number()})"


def show(arguments):
    made = current()
    if arguments.field == "version":
        print(made.version)
    elif arguments.field == "revision":
        print(made.revision)
    elif arguments.field == "build-number":
        print(made.build_number())
    else:
        print(f"version       {made.version}")
        print(f"revision      {made.revision}  (tells two uploads of one version apart)")
        print(f"build number  {made.build_number()}  (CFBundleVersion of the Mac and iOS apps)")
        print(f"source        {RELEASE_SOURCE} (productVersion, buildRevision)")
    return 0


def write_command(arguments):
    """`bump` and `sync`: write the places, then regenerate."""
    try:
        if arguments.command == "bump":
            old, new, changed = bump(ROOT, arguments.target, arguments.dry_run)
        else:
            old = new = current()
            changed = apply(ROOT, new, arguments.dry_run)
    except ValueError as failure:
        print(f"release: {failure}", file=sys.stderr)
        return 1
    verb = "would change" if arguments.dry_run else "changed"
    print(f"release: {describe(old)} -> {describe(new)}" if old != new else f"release: {describe(new)}")
    for path in changed:
        print(f"  {verb} {path}")
    if arguments.dry_run:
        return 0
    status = run_generator()
    if status:
        print("release: the generator failed after the places were written; fix the cause and run: uv run python scripts/release.py sync", file=sys.stderr)
    else:
        print("release: regenerated the outputs; nothing was tagged, pushed or built")
    return status


def check_command(_arguments):
    found = problems()
    for line in found:
        print(f"release: {line}", file=sys.stderr)
    if found:
        print(f"release: {len(found)} place(s) disagree; fix with: uv run python scripts/release.py bump <version>", file=sys.stderr)
        return 1
    # `contracts:generate-check` runs the same check as a gate step; it is repeated here so that
    # `check` alone answers whether everything agrees, and is cheap (under a second).
    generated = run_generator("--check")
    if not generated:
        print(f"release: every place says {describe(current())}")
    return generated


# The commands, by name. A packaging or notarization step is added here, as one function that
# reads `current()`; nothing below it needs to change.
COMMANDS = {"show": show, "bump": write_command, "sync": write_command, "check": check_command}


def parser():
    top = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = top.add_subparsers(dest="command", required=True)
    shown = commands.add_parser("show", help="print the version, the build number and where they come from")
    shown.add_argument("field", nargs="?", choices=SHOW_FIELDS, help="print only this value")
    bumped = commands.add_parser("bump", help="raise the version or the build revision everywhere and regenerate")
    bumped.add_argument(
        "target",
        help="major, minor, patch (each starts again at revision 0), revision (upload the same version again), "
        "or an explicit X.Y.Z above the current version",
    )
    bumped.add_argument("--dry-run", action="store_true", help="list the files that would change and write nothing")
    synced = commands.add_parser("sync", help="write the source's version and build number to every place, after an edit by hand")
    synced.add_argument("--dry-run", action="store_true", help="list the files that would change and write nothing")
    commands.add_parser("check", help="fail when a place disagrees with the source")
    return top


def main(argv=None):
    arguments = parser().parse_args(argv)
    return COMMANDS[arguments.command](arguments)


if __name__ == "__main__":
    sys.exit(main())
