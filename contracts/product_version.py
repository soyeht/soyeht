"""The product version: the one number every build of Soyeht carries (ADR 0115).

`contracts/release.json` holds it, with the build revision that tells two uploads of one version
apart. This module reads and validates them, derives the Mac and iOS build number, and renders
the constant the Swift clients send in `hello` and the revision the Windows resource carries.
The files that cannot be generated (the Cargo manifests and lockfiles, the npm package, the
Xcode projects) are written and checked by `scripts/release.py`, which imports the rules here.
"""

import json
import re
from typing import NamedTuple

from generated_header import header_text

RELEASE_SOURCE = "contracts/release.json"
SWIFT_OUTPUT = "contracts/swift/Sources/SoyehtProtocol/ProductVersion.swift"
WINDOWS_OUTPUT = "apps/desktop/packaging/windows/revision.h"
REQUIRED = ("productVersion",)
OPTIONAL = ("buildRevision",)
# The parts of a version below this stay in their own digits of the build number.
PART_LIMIT = 1000
# Re-uploads of one version: 0 to 99, two digits of the build number.
REVISION_LIMIT = 100
# A build number is a signed 32-bit integer for the tools that read it.
BUILD_NUMBER_LIMIT = 2**31
PATTERN = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
KINDS = ("major", "minor", "patch")
BUMPS = (*KINDS, "revision")


class Version(NamedTuple):
    major: int
    minor: int
    patch: int

    def __str__(self):
        return f"{self.major}.{self.minor}.{self.patch}"

    def bumped(self, kind):
        if kind == "major":
            return Version(self.major + 1, 0, 0)
        if kind == "minor":
            return Version(self.major, self.minor + 1, 0)
        if kind == "patch":
            return Version(self.major, self.minor, self.patch + 1)
        raise ValueError(f"cannot bump {kind}; use one of {', '.join(KINDS)}")


class Release(NamedTuple):
    """A version and the revision of its build: what one upload of the apps carries."""

    version: Version
    revision: int = 0

    def build_number(self):
        """The Mac and iOS build number (`CFBundleVersion`): one integer that only goes up.

        `(major * 1_000_000 + minor * 1_000 + patch) * 100 + revision`, so `0.1.0` revision 0 is
        100000 and `1.2.3` revision 4 is 100200304. It rises with the version and, for one
        version, with the revision, so an update check that compares build numbers agrees with
        one that compares versions. Any checkout yields the same number: nothing in it depends on
        git history, a clock or a counter that a second machine could disagree about.
        """
        major, minor, patch = self.version
        return ((major * PART_LIMIT + minor) * PART_LIMIT + patch) * REVISION_LIMIT + self.revision

    def bumped(self, kind):
        """The next release: a new version starts again at revision 0."""
        if kind == "revision":
            return release(self.version, self.revision + 1)
        return release(self.version.bumped(kind))


def check_revision(revision):
    if type(revision) is not int or not 0 <= revision < REVISION_LIMIT:
        raise ValueError(f"the build revision is a whole number from 0 to {REVISION_LIMIT - 1}, not {revision!r}")
    return revision


def parse(text):
    """A version from `MAJOR.MINOR.PATCH`: three plain numbers, no `v`, no pre-release text.

    Windows version resources, `CFBundleShortVersionString` and the Cargo, npm and Xcode
    manifests all take exactly this form, so it is the one the source allows.
    """
    if not isinstance(text, str) or not PATTERN.fullmatch(text):
        raise ValueError(f"a version is MAJOR.MINOR.PATCH, not {text!r}")
    version = Version(*(int(part) for part in text.split(".")))
    if max(version) >= PART_LIMIT:
        raise ValueError(f"each part of {text} must stay below {PART_LIMIT}")
    return version


def release(version, revision=0):
    """A release whose build number fits the 32 bits the tools that read it allow."""
    made = Release(version, check_revision(revision))
    if made.build_number() >= BUILD_NUMBER_LIMIT:
        raise ValueError(f"{version} revision {revision} has build number {made.build_number()}, which must stay below 2^31")
    return made


def validate(document):
    if not isinstance(document, dict) or not set(REQUIRED) <= set(document) <= set(REQUIRED + OPTIONAL):
        raise ValueError(f"release.json must have {', '.join(REQUIRED)} and may have {', '.join(OPTIONAL)}")
    return release(parse(document["productVersion"]), document.get("buildRevision", 0))


def load(text):
    return validate(json.loads(text))


def dump(made):
    document = {"productVersion": str(made.version), "buildRevision": made.revision}
    return json.dumps(document, indent=2) + "\n"


def render(made):
    header = f"// {header_text(RELEASE_SOURCE)}\n"
    return {
        SWIFT_OUTPUT: header
        + "public enum ProductVersion {\n"
        + "    /// The version of this build of Soyeht; clients send it as `clientVersion`.\n"
        + f'    public static let current = "{made.version}"\n'
        + "}\n",
        # The fourth field of the Windows version resource (`soyeht.rc`); the first three come
        # from the package version. A field is 16 bits wide, so the revision (0 to 99) fits.
        WINDOWS_OUTPUT: header + f"#define VERSION_REVISION {made.revision}\n",
    }
