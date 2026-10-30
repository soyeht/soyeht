"""What a release folder (`dist/`) must hold and prove before it is published (docs/release.md).

`release_flow.py collect` and `publish` use these checks; each returns the problems it found, one
line each, so a person fixes them all in one go. Nothing here signs or builds: the packagers do
(ADRs 0120, 0121 and 0123) and this only reads what they left.
"""

from __future__ import annotations

import struct
import subprocess
import sys
from pathlib import Path

from release_manifest import sha256_of

CHECKSUMS = "SHA256SUMS"
NOTES = "release-notes.md"
# The commit the packages were built from, written by `build` beside them.
BUILD_SHA = "BUILD_SHA"
DEVELOPER_ID = "Authority=Developer ID Application:"
# Offset of the data directories in the optional header of a PE32 and a PE32+ file, and the
# index of the certificate table (the Authenticode signature) among them.
DIRECTORIES = {0x10B: 96, 0x20B: 112}
CERTIFICATE_TABLE = 4

# The packages a complete release holds: (system, architecture, extension). A Linux build of the
# other architecture needs a host of that architecture and is not part of the first releases.
REQUIRED = (
    ("macos", "aarch64", "dmg"),
    ("linux", "x86_64", "deb"),
    ("linux", "x86_64", "AppImage"),
    ("windows", "x86_64", "exe"),
    ("windows", "aarch64", "exe"),
)


class MarkerError(Exception):
    """A `BUILD_SHA` that cannot be read as a commit id."""


def read_build_sha(path):
    """The commit id in a `BUILD_SHA` file. Windows PowerShell 5.1 writes redirected text as UTF-16
    with a byte order mark, so that and UTF-8 with a mark are read as well as plain ASCII."""
    data = Path(path).read_bytes()
    try:
        for mark, encoding in ((b"\xff\xfe", "utf-16"), (b"\xfe\xff", "utf-16"), (b"\xef\xbb\xbf", "utf-8-sig")):
            if data.startswith(mark):
                data = data.decode(encoding).encode()
                break
        text = data.decode("ascii").strip()
    except UnicodeDecodeError:
        raise MarkerError(f"{path} is not a commit id (not text)") from None
    if len(text) != 40 or any(c not in "0123456789abcdef" for c in text):
        raise MarkerError(f"{path} does not hold a full commit id")
    return text


def package_name(version, system, architecture, extension):
    return f"soyeht-{version}-{system}-{architecture}.{extension}"


def missing_packages(folder, version, without=()):
    """The required packages that are not in `folder`, leaving out the systems in `without`."""
    return [
        name
        for system, architecture, extension in REQUIRED
        if system not in without and not (folder / (name := package_name(version, system, architecture, extension))).is_file()
    ]


def write_checksums(folder, names):
    """`SHA256SUMS` in `shasum -a 256 -c` form for the given file names; returns its path."""
    path = folder / CHECKSUMS
    path.write_text("".join(f"{sha256_of(folder / name)}  {name}\n" for name in sorted(names)))
    return path


def checksum_problems(folder, names):
    """Each file of `names` against `SHA256SUMS`."""
    problems = []
    listed = {}
    try:
        lines = (folder / CHECKSUMS).read_text().splitlines()
    except OSError:
        return [f"{CHECKSUMS} is missing: run collect"]
    for line in lines:
        digest, _, name = line.partition("  ")
        listed[name] = digest
    for name in sorted(names):
        if name not in listed:
            problems.append(f"{name} is not in {CHECKSUMS}: run collect again")
        elif listed[name] != sha256_of(folder / name):
            problems.append(f"{name} does not match {CHECKSUMS}: it changed after collect")
    return problems


def sidecar_problems(folder):
    """Each `<package>.sha256` a packager wrote against its package: a copy that was damaged on the
    way from the machine that built it shows here."""
    problems = []
    for sidecar in sorted(folder.glob("soyeht-*.sha256")):
        target = folder / sidecar.name.removesuffix(".sha256")
        wanted = sidecar.read_text().split()[:1]
        if target.is_file() and wanted != [sha256_of(target)]:
            problems.append(f"{target.name} does not match {sidecar.name}: the copy is damaged")
    return problems


def pe_is_signed(path):
    """Whether a Windows program or installer carries an Authenticode signature.

    It reads the certificate table of the PE header, which `signtool` fills and nothing else does.
    That the certificate is valid and trusted is for `Get-AuthenticodeSignature` on Windows to say;
    `package_windows.py --require-signature` signs, and this refuses the file that kept no signature
    on its way here.
    """
    data = Path(path).read_bytes()
    if data[:2] != b"MZ" or len(data) < 0x40:
        return False
    (offset,) = struct.unpack_from("<I", data, 0x3C)
    if data[offset : offset + 4] != b"PE\0\0" or len(data) < offset + 28:
        return False
    (magic,) = struct.unpack_from("<H", data, offset + 24)
    directories = DIRECTORIES.get(magic)
    entry = None if directories is None else offset + 24 + directories + 8 * CERTIFICATE_TABLE
    if entry is None or len(data) < entry + 8:
        return False
    address, size = struct.unpack_from("<II", data, entry)
    return bool(address and size)


def windows_problems(folder, names):
    return [f"{name} carries no Authenticode signature" for name in sorted(names) if name.endswith(".exe") and not pe_is_signed(folder / name)]


def run_tool(command):
    return subprocess.run(command, capture_output=True, text=True)


def mac_problems(folder, names, tool=run_tool, system=None):
    """Each disk image must be signed with a Developer ID Application certificate, notarized
    (the ticket is stapled) and accepted by Gatekeeper; an ad-hoc build is none of these."""
    images = sorted(name for name in names if name.endswith(".dmg"))
    if not images:
        return []
    if (system or sys.platform) != "darwin":
        return [f"{name} cannot be checked on this system: collect on the Mac" for name in images]
    problems = []
    for name in images:
        path = str(folder / name)
        details = tool(["codesign", "-dvv", path]).stderr
        if DEVELOPER_ID not in details:
            problems.append(f"{name} is not signed with a Developer ID Application certificate")
        if tool(["xcrun", "stapler", "validate", path]).returncode:
            problems.append(f"{name} has no stapled notarization ticket")
        if tool(["spctl", "--assess", "-vv", "--type", "open", "--context", "context:primary-signature", path]).returncode:
            problems.append(f"{name} is rejected by Gatekeeper")
    return problems
