#!/usr/bin/env python3
"""Make a release from the packages of every system and publish it as a draft (docs/release.md).

    uv run python scripts/release_flow.py prepare TARGET [--dry-run]
    uv run python scripts/release_flow.py build [--mac] [--linux] [--windows] [--allow-unsigned] [--dry-run]
    uv run python scripts/release_flow.py collect [--from DIR ...] [--allow-unsigned] [--without SYSTEM ...]
    uv run python scripts/release_flow.py publish [--allow-unsigned] [--no-draft] [--dry-run]

Each packager runs on its own system (ADRs 0120, 0121 and 0123); this script only orders them:

  prepare  `release.py bump TARGET` (major, minor, patch, revision, X.Y.Z, or `current` for a first
           release) and the draft section of `CHANGELOG.md`. Nothing is committed: edit the
           changelog, commit, merge, and build from that commit of main.
  build    the Mac package here, the Linux packages on the CI host over ssh (the rootless script,
           only inside ~/soyeht-ci), and the command to run on the Windows host.
  collect  gathers the packages into dist/ (from `--from` folders too), checks that each system is
           there and signed, writes SHA256SUMS, the release notes from the changelog and the signed
           update manifest (`release_manifest.py`).
  publish  checks everything again and runs `gh release create` for the public repository as a
           DRAFT. A draft is not the "latest" release, so no client learns of it until the person
           publishes it. `--dry-run` prints the exact commands and runs none that changes anything.

A release refuses to go out without the update signing key, a Developer ID build of the Mac image
(signed and notarized) and signed Windows installers. `--allow-unsigned` is for a pre-release: it
lets any of those be missing and marks the release as a pre-release. Every secret is read from the
environment by the script that uses it; none is a file of this repository. This script never
creates a tag by itself: `gh release create` creates `v<version>` at the commit when the draft is
published.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

import changelog
import release
import release_checks
import release_manifest
import update_manifest  # the contracts directory is on the path once release is imported
from generated_profile import PROFILES
from package_common import PROFILE_NAME, ROOT

DIST = ROOT / "dist"
REPOSITORY = release_manifest.repository()
SYSTEMS = ("macos", "linux", "windows")
# The Linux CI host (docs/platforms.md, "A Linux gate host"): a mirror that the Mac pushes to and a
# clone of it. Everything the release does there stays below CI_FOLDER of the account's home.
LINUX_HOST_VARIABLE = "SOYEHT_RELEASE_LINUX_HOST"
RECEIVE_PACK_VARIABLE = "SOYEHT_RELEASE_RECEIVE_PACK"
DEFAULT_LINUX_HOST = "bignix"
CI_FOLDER = "soyeht-ci"
MIRROR = f"{CI_FOLDER}/mirror.git"
CLONE = f"{CI_FOLDER}/repo"
TREES = f"{CI_FOLDER}/release"
FULL_SHA = re.compile(r"^[0-9a-f]{40}$")
# No leading dash: a value that starts with one would be read as an option by ssh, scp and git.
HOST_NAME = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._@-]*$")


class Refused(Exception):
    """A step that cannot go on; the message says what to fix."""


class Runner:
    """Runs the commands that change something, or only prints them (`--dry-run`)."""

    def __init__(self, dry_run=False):
        self.dry_run = dry_run

    def run(self, command, *, stdin=None, cwd=ROOT):
        print("+ " + " ".join(shlex.quote(str(part)) for part in command), flush=True)
        if stdin is not None:
            print("  with this script on its input:\n" + "\n".join("    " + line for line in stdin.splitlines()), flush=True)
        if self.dry_run:
            print("  (dry run: not run)", flush=True)
            return 0
        return subprocess.run([str(part) for part in command], input=stdin, text=True, cwd=cwd).returncode

    def must(self, command, **options):
        status = self.run(command, **options)
        if status:
            raise Refused(f"{command[0]} failed with status {status}")


def probe(command, cwd=ROOT):
    """A command that only reads: (status, output). A tool that is not installed is status 127."""
    try:
        result = subprocess.run(command, capture_output=True, text=True, cwd=cwd, timeout=120)
    except (OSError, subprocess.TimeoutExpired) as failure:
        return 127, str(failure)
    return result.returncode, result.stdout


def head(probe=probe):
    status, output = probe(["git", "rev-parse", "HEAD"])
    if status or not FULL_SHA.match(output.strip()):
        raise Refused("cannot read the current commit")
    return output.strip()


def source_problems(probe=probe):
    """What stops a release from this checkout: local changes, or a commit main does not hold."""
    found = []
    status, output = probe(["git", "status", "--porcelain"])
    if status or output.strip():
        found.append("the working tree has changes: commit them (or build from a clean checkout)")
    probe(["git", "fetch", "origin", "main"])
    status, _ = probe(["git", "merge-base", "--is-ancestor", "HEAD", "origin/main"])
    if status:
        found.append("HEAD is not on origin/main: merge the version bump first and build from main")
    found += release.problems()
    return found


# --- prepare ------------------------------------------------------------------------------------


def prepare(options):
    old = release.current()
    if options.target == "current":
        new = old
        print(f"release: keeping {release.describe(old)}")
    else:
        try:
            new = release.next_release(old, options.target)
        except ValueError as failure:
            raise Refused(str(failure)) from None
        if release.main(["bump", options.target, *(["--dry-run"] if options.dry_run else [])]):
            raise Refused("the version could not be raised")
    draft = ["draft", "--version", str(new.version)]
    if options.dry_run:
        print("release: dry run: the changelog section would be:\n")
        changelog.main([*draft, "--print"])
        return 0
    # `--replace` writes over a draft only: a section a person has edited is never discarded.
    if changelog.main([*draft, "--replace"]):
        raise Refused("the changelog draft could not be written")
    print(
        "\nNext: edit CHANGELOG.md (delete the draft line when the entries read well for a user), commit "
        "the bump and the changelog in a pull request, merge it, and build from that commit of main."
    )
    return 0


# --- build --------------------------------------------------------------------------------------

TARGETS = {"macos": "mac", "linux": "linux", "windows": "windows"}


def linux_script(version, sha):
    """The script the Linux host runs (on its input, in a login shell): the release commit in a
    folder of its own below ~/soyeht-ci, built by the rootless script."""
    if not FULL_SHA.match(sha):
        raise Refused(f"{sha!r} is not a full commit id")
    return "\n".join(
        [
            "set -euo pipefail",
            f'tree="$HOME/{TREES}/{sha}"',
            f'cd "$HOME/{CLONE}"',
            "git fetch origin",
            f'if [ ! -d "$tree" ]; then git worktree add --detach "$tree" {sha}; fi',
            'cd "$tree"',
            f'test "$(git rev-parse HEAD)" = "{sha}"',
            "nice -n 15 nix-shell -p bubblewrap curl git gnutar coreutils --run "
            "'bash scripts/package_linux_rootless.sh --format all'",
            f'ls -l "$tree"/dist/soyeht-{version}-linux-*',
        ]
    )


def build_linux(runner, host, version, sha, destination, environment):
    if not HOST_NAME.match(host):
        raise Refused(f"{host!r} is not a host name")
    push = ["git", "push"]
    if environment.get(RECEIVE_PACK_VARIABLE):
        push.append(f"--receive-pack={environment[RECEIVE_PACK_VARIABLE]}")
    runner.must([*push, f"{host}:{MIRROR}", f"{sha}:refs/heads/release-{version}"])
    runner.must(["ssh", "-o", "BatchMode=yes", host, "bash", "-l", "-s"], stdin=linux_script(version, sha))
    runner.must(["scp", "-o", "BatchMode=yes", f"{host}:{TREES}/{sha}/dist/soyeht-{version}-linux-*", str(destination) + "/"])


def windows_instructions(version, sha):
    return "\n".join(
        [
            "Windows: run this on the Windows host (docs/release.md, \"The Windows installers\"), in a clone:",
            f"    git fetch origin && git checkout {sha}",
            "    uv run python scripts\\package_windows.py --require-signature",
            f"    git rev-parse HEAD | Out-File -Encoding ascii dist\\{release_checks.BUILD_SHA}",
            f"then copy dist\\soyeht-{version}-windows-*.exe and dist\\{release_checks.BUILD_SHA} to a folder here and pass it to collect with --from.",
            "Set the SOYEHT_WINDOWS_SIGN_* variables first (docs/platforms.md, \"Windows packages\").",
        ]
    )


def refuse_or_warn(found, what, dry_run):
    """A dry run lists what would stop the real one and goes on to show the commands."""
    if not found:
        return 0
    text = f"{what}:\n  " + "\n  ".join(found)
    if not dry_run:
        raise Refused(text)
    print(f"dry run: would refuse, {text}\n", flush=True)
    return 1


def marker_sha(path):
    try:
        return release_checks.read_build_sha(path)
    except release_checks.MarkerError as failure:
        raise Refused(str(failure)) from None


def build_folders(folder):
    """The folders this build's packages land in: `dist/` and, for an ad-hoc Mac build, `dist/ad-hoc/`."""
    return [folder, ad_hoc_folder(folder)]


def ad_hoc_folder(folder):
    return folder / "ad-hoc"


def stale_build_problems(folder, version, sha):
    """Packages left by a build of another commit would be taken for this one's."""
    found = []
    for place in build_folders(folder):
        marker = place / release_checks.BUILD_SHA
        recorded = marker_sha(marker) if marker.is_file() else None
        if package_names(place, version) and recorded != sha:
            found.append(f"{place} holds packages {'of commit ' + recorded[:9] if recorded else 'with no ' + release_checks.BUILD_SHA}: "
                         "clear it (they were built from another commit)")
    return found


def mark_build(folder, sha, dry_run):
    """Write the commit the packages were built from beside them (`BUILD_SHA`), after they exist."""
    for place in build_folders(folder):
        if place != folder and not place.is_dir():
            continue
        marker = place / release_checks.BUILD_SHA
        if dry_run:
            print(f"dry run: would write {marker} ({sha[:9]})")
        else:
            place.mkdir(parents=True, exist_ok=True)
            marker.write_text(sha + "\n")


def build(options, runner=None, probe=probe, environment=os.environ):
    runner = runner or Runner(options.dry_run)
    chosen = [system for system, flag in TARGETS.items() if getattr(options, flag)] or list(TARGETS)
    status = refuse_or_warn(source_problems(probe), "not a release checkout", options.dry_run)
    version, sha = str(release.current().version), head(probe)
    status |= refuse_or_warn(stale_build_problems(options.dist, version, sha), "cannot build here", options.dry_run)
    if "macos" in chosen:
        command = ["nice", "-n", "15", "uv", "run", "python", "scripts/package_mac.py", "--dist", str(options.dist)]
        runner.must(command if options.allow_unsigned else [*command, "--release"])
    if "linux" in chosen:
        host = options.linux_host or environment.get(LINUX_HOST_VARIABLE) or DEFAULT_LINUX_HOST
        build_linux(runner, host, version, sha, options.dist, environment)
    if "windows" in chosen:
        print(windows_instructions(version, sha))
    mark_build(options.dist, sha, options.dry_run)
    print(f"\nbuild: {version} at {sha[:9]}: {', '.join(chosen)}. Next: collect")
    return status


# --- collect ------------------------------------------------------------------------------------


def gather(folder, version, sources):
    """Copy the packages of `version` from each source folder into `folder`.

    A folder made elsewhere (the Windows installers) must say, in `BUILD_SHA`, that its packages
    were built from the commit of the rest; so must the `dist/ad-hoc` folder of an ad-hoc Mac build."""
    copied = []
    for source in sources:
        if not source.is_dir() or source.resolve() == folder.resolve():
            continue
        if not any(path.is_file() for path in source.glob("soyeht-*")):
            continue
        theirs = source / release_checks.BUILD_SHA
        ours = folder / release_checks.BUILD_SHA
        if not theirs.is_file():
            raise Refused(f"{source} has no {release_checks.BUILD_SHA}: copy it from the machine that built the packages")
        if not ours.is_file() or marker_sha(theirs) != marker_sha(ours):
            raise Refused(f"{source} holds packages built from {marker_sha(theirs)[:9]}, not from the "
                          f"commit of {folder}: build them again")
        for path in sorted(source.glob("soyeht-*")):
            if not path.is_file():
                continue
            if not path.name.startswith(f"soyeht-{version}-"):
                raise Refused(f"{path} is not a package of {version}")
            shutil.copy2(path, folder / path.name)
            copied.append(path.name)
    return copied


def all_package_names(folder):
    """Every `soyeht-<version>-...` package in `folder`, of any version, not the checksums beside them."""
    if not folder.is_dir():
        return []
    return sorted(
        path.name
        for path in folder.iterdir()
        if path.is_file()
        and release_manifest.PACKAGE.fullmatch(path.name)
        and not path.name.endswith(release_manifest.SIDECAR_SUFFIXES)
    )


def package_names(folder, version):
    """The packages of `version` in `folder`: one of another version is never collected or uploaded."""
    return [name for name in all_package_names(folder) if name.startswith(f"soyeht-{version}-")]


def stale_problems(folder, version):
    return [
        f"{name} is a package of another version than {version}: remove it from {folder}"
        for name in all_package_names(folder)
        if not name.startswith(f"soyeht-{version}-")
    ]


def fetch_text(address):
    """The text at a public address, or None when there is none (404)."""
    try:
        with urllib.request.urlopen(address, timeout=20) as response:
            return response.read(release_manifest.POLICY["manifest"]["maxBytes"] + 1).decode()
    except urllib.error.HTTPError as failure:
        if failure.code == 404:
            return None
        raise Refused(f"cannot read the last published manifest (status {failure.code}); pass --last-sequence N") from None
    except (urllib.error.URLError, OSError, UnicodeDecodeError):
        raise Refused(
            "cannot read the last published manifest (no network?); pass --last-sequence N, "
            "or --first-release when nothing was published"
        ) from None


def last_published_sequence(options, fetch=fetch_text):
    """The sequence of the manifest the clients read now, None when there is no release yet."""
    if options.first_release:
        return None
    if options.last_sequence is not None:
        return options.last_sequence
    text = fetch(release_manifest.POLICY["manifest"]["url"])
    return None if text is None else release_manifest.previous_sequence(text, release_manifest.trusted_keys([]))


def signing_problems(folder, names, options, tool=release_checks.run_tool):
    """The Developer ID, notarization and Authenticode checks; none for `--allow-unsigned`."""
    if options.allow_unsigned:
        return []
    return release_checks.mac_problems(folder, names, tool=tool) + release_checks.windows_problems(folder, names)


def changelog_problems(version):
    try:
        changelog.notes_of(changelog.CHANGELOG.read_text(), version)
    except (changelog.ChangelogError, OSError) as failure:
        return [str(failure)]
    return []


def collect(options, environment=os.environ, fetch=fetch_text, tool=release_checks.run_tool, probe=probe):
    version = str(release.current().version)
    found = [f"{line} (release.py check)" for line in release.problems()]
    folder = options.dist
    folder.mkdir(parents=True, exist_ok=True)
    if options.without and not options.allow_unsigned:
        raise Refused("--without leaves a system out of the release: it is for a pre-release (--allow-unsigned)")
    for name in gather(folder, version, [*options.sources, *([ad_hoc_folder(folder)] if options.allow_unsigned else [])]):
        print(f"collect: copied {name}")
    names = package_names(folder, version)
    found += stale_problems(folder, version)
    found += build_sha_problems(folder, probe)
    found += [f"{name} is missing" for name in release_checks.missing_packages(folder, version, options.without)]
    found += release_checks.sidecar_problems(folder)
    found += signing_problems(folder, names, options, tool)
    found += changelog_problems(version)
    asset = release_manifest.POLICY["manifest"]["asset"]
    signed = bool(environment.get(release_manifest.KEY_ENVIRONMENT))
    if not signed and not options.allow_unsigned:
        found.append(f"{release_manifest.KEY_ENVIRONMENT} is not set: the update manifest cannot be signed")
    if found:
        raise Refused("the release is not ready:\n  " + "\n  ".join(found))
    (folder / release_checks.NOTES).write_text(changelog.notes_of(changelog.CHANGELOG.read_text(), version))
    assets = list(names)
    if signed:
        try:
            text, _ = release_manifest.build(
                folder, environment, options.minimum, sequence=options.sequence,
                last_sequence=last_published_sequence(options, fetch),
            )
        except release_manifest.Refused as refused:
            raise Refused(str(refused)) from None
        (folder / asset).write_text(text)
        assets.append(asset)
    else:
        print("collect: no update signing key: this pre-release has no update manifest")
    release_checks.write_checksums(folder, assets)
    print(f"collect: {len(assets)} files in {folder}, with {release_checks.CHECKSUMS} and {release_checks.NOTES}")
    for name in assets:
        print(f"  {name}")
    return 0


# --- publish ------------------------------------------------------------------------------------


def publish_files(folder, version):
    """The assets of the release in the order they are uploaded: the manifest last."""
    asset = release_manifest.POLICY["manifest"]["asset"]
    extra = [folder / name for name in (release_checks.CHECKSUMS, asset) if (folder / name).is_file()]
    return [*(folder / name for name in package_names(folder, version)), *extra]


def manifest_problems(folder, version, fetch, options):
    """The manifest in `folder` as a client reads it: signed by a key of the contract, for this
    version, listing exactly the packages that are here, with their hashes, and above the last one
    published."""
    asset = release_manifest.POLICY["manifest"]["asset"]
    try:
        payload = update_manifest.check_payload(
            update_manifest.open_envelope((folder / asset).read_text(), release_manifest.trusted_keys([]))
        )
    except update_manifest.Failed as failure:
        return [f"{asset} is refused as a client would refuse it ({failure.failure}): collect again"]
    found = [] if payload["version"] == version else [f"{asset} is for {payload['version']}, not {version}"]
    for package in payload["packages"]:
        path = folder / package["url"].rsplit("/", 1)[1]
        if not path.is_file() or release_checks.sha256_of(path) != package["sha256"] or path.stat().st_size != package["size"]:
            found.append(f"{asset} lists {path.name} with a hash or size that is not the file's: collect again")
    last = last_published_sequence(options, fetch)
    if last is not None and payload["sequence"] <= last:
        found.append(f"{asset} has sequence {payload['sequence']}, not above {last}, the last published: collect again")
    return found


def publish_problems(options, files, probe=probe, fetch=fetch_text, tool=release_checks.run_tool):
    folder, version = options.dist, str(release.current().version)
    names = [path.name for path in files]
    listed = [name for name in names if name != release_checks.CHECKSUMS]
    found = source_problems(probe)
    if options.without and not options.allow_unsigned:
        found.append("--without leaves a system out of the release: it is for a pre-release (--allow-unsigned)")
    found += [f"{name} is missing: run collect" for name in release_checks.missing_packages(folder, version, options.without)]
    if (folder / release_checks.CHECKSUMS).is_file():
        found += release_checks.checksum_problems(folder, listed)
    else:
        found.append(f"{release_checks.CHECKSUMS} is missing: run collect")
    found += stale_problems(folder, version)
    found += build_sha_problems(folder, probe)
    found += signing_problems(folder, package_names(folder, version), options, tool)
    found += changelog_problems(version)
    if not (folder / release_checks.NOTES).is_file():
        found.append(f"{release_checks.NOTES} is missing: run collect")
    asset = release_manifest.POLICY["manifest"]["asset"]
    if asset in names:
        found += manifest_problems(folder, version, fetch, options)
    elif not options.allow_unsigned:
        found.append(f"{asset} is missing: set {release_manifest.KEY_ENVIRONMENT} and run collect")
    found += existing_release_problems(release_manifest.release_tag(version), probe)
    return found


def existing_release_problems(tag, probe=probe):
    """`gh release list` shows drafts as well as public releases (a draft has no tag to view yet)."""
    status, output = probe(["gh", "release", "list", "--repo", REPOSITORY, "--limit", "1000", "--json", "tagName,isDraft"])
    try:
        listed = json.loads(output) if status == 0 else None
    except ValueError:
        listed = None
    if not isinstance(listed, list):
        return [f"cannot list the releases of {REPOSITORY} (gh login?): publishing blind could duplicate {tag}"]
    if any(isinstance(item, dict) and item.get("tagName") == tag for item in listed):
        return [f"the release {tag} already exists in {REPOSITORY} (a draft counts): raise the version, or delete it by hand"]
    return []


def build_sha_problems(folder, probe=probe):
    """The packages must have been built from the commit that is tagged."""
    marker = folder / release_checks.BUILD_SHA
    if not marker.is_file():
        return [f"{release_checks.BUILD_SHA} is missing: the packages are not tied to a commit; run build"]
    try:
        built = release_checks.read_build_sha(marker)
    except release_checks.MarkerError as failure:
        return [f"{failure}: build again"]
    current = head(probe)
    if built != current:
        return [f"the packages were built from {built[:9]}, not from {current[:9]}, the commit to tag: build again"]
    return []


def create_command(options, files, sha):
    version = str(release.current().version)
    title = f"{PROFILES[PROFILE_NAME]['displayNames']['window']} {version}"
    command = ["gh", "release", "create", release_manifest.release_tag(version), "--repo", REPOSITORY, "--title", title,
               "--notes-file", str(options.dist / release_checks.NOTES), "--target", sha]
    if not options.no_draft:
        command.append("--draft")
    if options.allow_unsigned:
        command.append("--prerelease")
    return [*command, *map(str, files)]


def publish(options, runner=None, probe=probe, fetch=fetch_text, tool=release_checks.run_tool):
    runner = runner or Runner(options.dry_run)
    files = publish_files(options.dist, str(release.current().version))
    status = refuse_or_warn(publish_problems(options, files, probe, fetch, tool), "will not publish", options.dry_run)
    runner.must(create_command(options, files, head(probe)))
    tag = release_manifest.release_tag(str(release.current().version))
    if options.no_draft:
        print(f"\npublish: {tag} is public; the clients see it now")
    else:
        print(f"\npublish: {tag} is a draft. Check its assets, then make it public with:\n"
              f"  gh release edit {tag} --repo {REPOSITORY} --draft=false")
    return status


# --- the command line ---------------------------------------------------------------------------


def parser():
    top = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = top.add_subparsers(dest="command", required=True)

    def common(command, dry_run=False):
        command.add_argument("--dist", type=Path, default=DIST, help="the release folder (default: dist/)")
        command.add_argument("--allow-unsigned", action="store_true",
                             help="a pre-release: allow an ad-hoc Mac build, unsigned Windows installers and no update key")
        if dry_run:
            command.add_argument("--dry-run", action="store_true", help="print the commands that change something and run none")

    def sequence(command):
        command.add_argument("--last-sequence", type=int, help="the sequence of the manifest published before (default: read it)")
        command.add_argument("--first-release", action="store_true", help="nothing was published before")

    prepared = commands.add_parser("prepare", help="raise the version and draft the changelog section")
    prepared.add_argument("target", help="major, minor, patch, revision, X.Y.Z, or `current` to keep the version (a first release)")
    prepared.add_argument("--dry-run", action="store_true")
    built = commands.add_parser("build", help="build the Mac package here, the Linux ones on the CI host")
    common(built, True)
    for name, text in (("mac", "the Mac package"), ("linux", "the Linux packages"), ("windows", "show the Windows command")):
        built.add_argument(f"--{name}", action="store_true", help=f"only {text}")
    built.add_argument("--linux-host", help=f"the Linux CI host (default: ${LINUX_HOST_VARIABLE}, else {DEFAULT_LINUX_HOST})")
    collected = commands.add_parser("collect", help="gather, check and checksum the packages and sign the update manifest")
    common(collected)
    sequence(collected)
    collected.add_argument("--from", dest="sources", action="append", default=[], type=Path, metavar="DIR",
                           help="a folder of packages made elsewhere (the Windows installers)")
    collected.add_argument("--without", action="append", default=[], choices=tuple(TARGETS),
                           help="leave a system out (a pre-release only)")
    collected.add_argument("--sequence", type=int, help="the manifest's sequence (default: the clock; see docs/release.md)")
    collected.add_argument("--minimum", default="0.0.0", help="the oldest version still supported (default: all)")
    published = commands.add_parser("publish", help="create the GitHub release as a draft")
    common(published, True)
    sequence(published)
    published.add_argument("--no-draft", action="store_true", help="publish at once (the clients see it)")
    published.add_argument("--without", action="append", default=[], choices=tuple(TARGETS), help="a system the release leaves out")
    return top


def main(argv=None):
    options = parser().parse_args(argv)
    try:
        return {"prepare": prepare, "build": build, "collect": collect, "publish": publish}[options.command](options)
    except (Refused, changelog.ChangelogError, release_manifest.Refused) as failure:
        print(f"release_flow: {failure}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
