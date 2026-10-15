#!/usr/bin/env python3
"""Fail when a file that does not belong in the repository is tracked or was added.

Archives, native binaries, logs and patch leftovers have no place in the tree, and a
file that is committed stays in history for every clone even after a later commit
removes it. The check therefore reads two sets of files:

- every tracked file, and
- every file that any commit in `BASE..HEAD` added, including one a later commit in
  the same range deleted.

A file fails when it:

- has a forbidden extension (`FORBIDDEN_EXTENSIONS`) or name (`FORBIDDEN_NAMES`);
- starts with the signature of an archive or a native executable, whatever its name;
- is binary (has a NUL byte in its first 8000 bytes) and sits outside the directories
  that `BINARY_HOMES` lists for its extension, and is not in `ALLOWED_FILES`;
- is larger than `MAX_BYTES`.

`ALLOWED_FILES` maps a path to the reason it is allowed. An entry for a file that no
longer exists fails, so the list cannot go stale.
"""

from __future__ import annotations

import argparse
import posixpath
import subprocess
from pathlib import Path
from typing import NamedTuple

from gate_base import BaseError, resolve_base

ROOT = Path(__file__).resolve().parents[1]
MAX_BYTES = 1_000_000
SNIFF_BYTES = 8000

FORBIDDEN_EXTENSIONS = {
    ".tar", ".tgz", ".gz", ".bz2", ".xz", ".zip", ".7z", ".rar", ".jar",
    ".dmg", ".pkg", ".iso", ".ipa", ".apk",
    ".a", ".so", ".dylib", ".dll", ".exe", ".o", ".obj", ".lib", ".class", ".bin",
    ".log", ".orig", ".rej", ".swp", ".pyc",
}  # fmt: skip
FORBIDDEN_NAMES = {".DS_Store"}
# The first bytes of an archive or native executable, however the file is named.
SIGNATURES = {
    b"\x1f\x8b": "gzip archive",
    b"PK\x03\x04": "zip archive",
    b"BZh": "bzip2 archive",
    b"\xfd7zXZ\x00": "xz archive",
    b"7z\xbc\xaf\x27\x1c": "7z archive",
    b"!<arch>\n": "static library",
    b"\x7fELF": "ELF executable",
    b"\xfe\xed\xfa\xce": "Mach-O executable",
    b"\xfe\xed\xfa\xcf": "Mach-O executable",
    b"\xce\xfa\xed\xfe": "Mach-O executable",
    b"\xcf\xfa\xed\xfe": "Mach-O executable",
    b"\xca\xfe\xba\xbe": "universal executable or Java class",
}
# A binary file is allowed only under one of these directories, by extension.
BINARY_HOMES = {
    ".png": (
        "docs/reports/",
        "apps/mac/Resources/Assets.xcassets/",
        "apps/ios/Resources/Assets.xcassets/",
        "apps/shared/Sources/SoyehtClient/Resources/",
    ),
    ".ttf": (
        "apps/mac/Resources/Fonts/",
        "apps/desktop/crates/soyeht-desktop/assets/fonts/",
    ),
    ".pen": ("design/",),
}
ALLOWED_FILES = {
    "scripts/human-mode-spike/test.mp4": (
        "the 2 KB clip that docs/reports/human-mode-spike.md describes and its script plays"
    ),
}


class Added(NamedTuple):
    """A file as one commit added it: its path, blob and the commit's short name."""

    path: str
    oid: str
    commit: str


def git(root: Path, *args: str) -> bytes:
    result = subprocess.run(["git", *args], cwd=root, capture_output=True)
    if result.returncode:
        raise BaseError(f"git {' '.join(args)} failed: {result.stderr.decode().strip()}")
    return result.stdout


def tracked(root: Path) -> list[str]:
    return [name for name in git(root, "ls-files", "-z").decode().split("\0") if name]


def added_in_range(root: Path, base: str) -> list[Added]:
    """Return every file a commit of BASE..HEAD added, with the blob it added."""
    output = git(
        root,
        "log",
        "--diff-filter=A",
        "--raw",
        "--no-abbrev",
        "-z",
        "--format=%x01%h",
        f"{base}..HEAD",
    ).decode()
    found: list[Added] = []
    for chunk in output.split("\x01")[1:]:
        fields = chunk.split("\0")
        commit = fields[0].strip()[:7]
        # A raw entry is `:oldmode newmode oldoid newoid A`, then the path as its own field.
        for index in range(1, len(fields) - 1):
            meta = fields[index].lstrip("\n").split()
            if len(meta) == 5 and meta[0].startswith(":"):
                found.append(Added(fields[index + 1], meta[3], commit))
    return found


def problems_for(path: str, head: bytes, size: int) -> list[str]:
    """Return what is wrong with one file, given its first bytes and size."""
    problems = [f"{size} bytes is over the {MAX_BYTES}-byte limit"] if size > MAX_BYTES else []
    if path in ALLOWED_FILES:
        return problems
    name = posixpath.basename(path)
    suffix = posixpath.splitext(name)[1].lower()
    kind = [why for sig, why in SIGNATURES.items() if head.startswith(sig)]
    if head.startswith(b"MZ") and b"\0" in head[:64]:
        kind.append("Windows executable")
    if suffix in FORBIDDEN_EXTENSIONS or name in FORBIDDEN_NAMES:
        problems.append(f"{suffix or name} files do not belong in the repository")
    problems += [f"starts like a {why}" for why in kind]
    homes = BINARY_HOMES.get(suffix, ())
    is_binary = b"\0" in head[:SNIFF_BYTES]
    if is_binary and not kind and suffix not in FORBIDDEN_EXTENSIONS and not path.startswith(homes):
        where = ", ".join(homes) or "nowhere yet"
        problems.append(
            f"binary {suffix or 'file'} outside its allowed directories ({where}); "
            "keep it out of the tree, or extend BINARY_HOMES or ALLOWED_FILES with a reason"
        )
    return problems


def tree_problems(root: Path) -> list[str]:
    found: list[str] = []
    names = tracked(root)
    for name in names:
        path = root / name
        if not path.is_file():
            continue
        with path.open("rb") as handle:
            head = handle.read(SNIFF_BYTES)
        found += [f"{name}: {why}" for why in problems_for(name, head, path.stat().st_size)]
    for name, reason in ALLOWED_FILES.items():
        if name not in names:
            found.append(f"{name}: stale ALLOWED_FILES entry ({reason}); remove it")
    return found


def history_problems(root: Path, base: str) -> list[str]:
    found: list[str] = []
    present = set(tracked(root))
    for added in added_in_range(root, base):
        blob = git(root, "cat-file", "blob", added.oid)
        problems = problems_for(added.path, blob[:SNIFF_BYTES], len(blob))
        gone = "" if added.path in present else " (deleted later in the range, but kept in history)"
        found += [f"{added.path}: added in {added.commit}{gone}: {why}" for why in problems]
    return found


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", help="full commit OID to compare with; default is the merge-base")
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    args, _ = parser.parse_known_args()
    root = args.root.resolve()
    try:
        base = resolve_base(root, args.base)
        found = tree_problems(root) + history_problems(root, base)
    except BaseError as error:
        print(f"check_repo_files: {error}")
        return 2
    for line in dict.fromkeys(found):
        print(line)
    if found:
        print(f"{len(found)} problems with files in the repository")
    return 1 if found else 0


if __name__ == "__main__":
    raise SystemExit(main())
