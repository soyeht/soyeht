"""The identity rules of a signed Mac release, so an update never adds a second row to the
privacy lists of System Settings (ADR 0120, "Privacy lists").

macOS keys a grant (Files and Folders, Automation, App Management, Local Network, ...) by the
program's path plus the designated requirement it was stored with. An update keeps the row only if
the path is the same and the new program still satisfies the stored requirement. A requirement made
of a code directory hash (an ad-hoc signature, or a bare program signed with no `--identifier`)
never matches the next build, so every update would add a row. These functions read the text that
`codesign -dvv` and `codesign -d -r-` print and say what is wrong; they run nothing, so the tests
feed them recorded output. `package_mac.verify_bundle` runs them on every program of the bundle.
"""

from __future__ import annotations

import re
from pathlib import PurePath

# Anything that names a version: `1.2`, `0.3.1`, `2.0.0-rc.1`.
VERSION = re.compile(r"(?<![A-Za-z0-9])v?\d+\.\d+(?:\.\d+)*")
# A hex run that looks like a hash (a commit, a digest), not a word.
HASH = re.compile(r"(?<![0-9A-Za-z])(?=[0-9a-f]*\d)(?=[0-9a-f]*[a-f])[0-9a-f]{8,}(?![0-9A-Za-z])")
CDHASH = re.compile(r'cdhash\s+H"')
TEAM_PIN = re.compile(r'certificate leaf\[subject\.OU\]\s*=\s*"?([A-Z0-9]{10})"?')
LEAF_PIN = re.compile(r"certificate leaf\[subject\.(?:OU|CN)\]\s*=")
# The folder macOS puts a framework's versions in (`Versions/A`): not a release version.
FRAMEWORK_LAYOUT = {"Versions", "Current"}
LIBRARY_SUFFIX = ".dylib"


def requirement_of(text):
    """The designated requirement in the output of `codesign -d -r-`, or None when it has none."""
    for line in text.splitlines():
        # An ad-hoc signature has no requirement of its own: codesign prints the implied one as
        # a comment (`# designated => cdhash H"..."`).
        if line.lstrip("# ").startswith("designated =>"):
            return line.split("=>", 1)[1].strip()
    return None


def field_of(details, name):
    """The value of a `Name=value` line of `codesign -dvv`, or None."""
    for line in details.splitlines():
        if line.startswith(f"{name}="):
            return line.split("=", 1)[1]
    return None


def requirement_problems(requirement, identifier, team):
    """What is wrong with a designated requirement for a program signed as `identifier`.

    It must name the identifier and an Apple-issued certificate pinned to the signer (the team's
    OU, or the leaf's name), and never a code hash. With `team`, the OU must be that team.
    """
    if requirement is None:
        return ["no designated requirement"]
    problems = []
    if CDHASH.search(requirement):
        problems.append("the designated requirement is a code hash (ad-hoc): every update is a new row")
    if f'identifier "{identifier}"' not in requirement:
        problems.append(f'the designated requirement does not name identifier "{identifier}"')
    if "anchor apple generic" not in requirement:
        problems.append("the designated requirement has no Apple anchor")
    elif not LEAF_PIN.search(requirement):
        problems.append("the designated requirement pins no signer: any Apple-issued certificate would match")
    pinned = TEAM_PIN.search(requirement)
    # An Apple Development certificate pins the leaf's name, not the team; the team is then
    # checked on the signature itself (`team_problems`).
    if team and pinned and pinned.group(1) != team:
        problems.append(f"the designated requirement pins team {pinned.group(1)}, not {team}")
    return problems


def team_problems(details, team):
    """The team on the signature itself; an ad-hoc signature has none."""
    found = field_of(details, "TeamIdentifier")
    if found in (None, "not set"):
        return ["the signature has no team (ad hoc)"]
    return [f"team is {found}, not {team}"] if team and found != team else []


def identifier_problems(identifier, expected, prefix, executable):
    """The signing identifier. A program the system can run must carry the profile's prefix; a
    known program must carry exactly its own."""
    if not identifier:
        return ["no signing identifier"]
    if expected and identifier != expected:
        return [f"identifier is {identifier}, not {expected}"]
    if executable and not identifier.startswith(f"{prefix}."):
        return [f"identifier {identifier} does not start with {prefix}."]
    return []


def path_problems(relative, version=None):
    """A program's place inside the bundle must not move with a release: no version, no hash.

    `relative` is the path below the `.app`, the bundle's own name first. Libraries may carry their
    version in their file name (`libc++.1.dylib`): they are not a program the system runs.
    """
    parts = PurePath(relative).parts
    problems = []
    for index, part in enumerate(parts):
        if part in FRAMEWORK_LAYOUT or (index == len(parts) - 1 and part.endswith(LIBRARY_SUFFIX)):
            continue
        if VERSION.search(part) or (version and str(version) in part):
            problems.append(f"the path component {part!r} holds a version")
        if HASH.search(part):
            problems.append(f"the path component {part!r} holds a hash")
    return problems


def code_problems(relative, *, details, requirement, identifier, prefix, team, executable, version=None):
    """Everything wrong with one program of the bundle: its path, identifier, team and requirement.

    `identifier` is what the packaging script signs it as (None for a library that keeps the name
    codesign derives). `executable` is true for the app and the programs next to it.
    """
    found = field_of(details, "Identifier")
    problems = path_problems(relative, version)
    problems += identifier_problems(found, identifier, prefix, executable)
    problems += team_problems(details, team)
    problems += requirement_problems(requirement, found or "", team)
    return problems
