#!/usr/bin/env python3
"""Build the Windows installers of the production profile (ADR 0123).

    uv run python scripts\\package_windows.py [--arch x86_64|aarch64|all] [--skip-build]
        [--engine-target DIR] [--desktop-target DIR] [--out DIR] [--inno ISCC.exe]
        [--signtool signtool.exe] [--require-signature]

It builds the release programs of both cargo workspaces for `x86_64-pc-windows-msvc` and
`aarch64-pc-windows-msvc` (either host builds both), with `SOYEHT_BUILD_PROFILE=prod` so the programs
find the production profile of `contracts/profiles.json` without setup (ADR 0112), checks that each
program is for the right processor and the right subsystem, signs them when asked to, and has Inno
Setup compile `apps/desktop/packaging/windows/soyeht.iss` into
`dist/soyeht-<version>-windows-<arch>.exe`. The version is `contracts/release.json` and nothing else
(ADR 0115). Run it on Windows, where the programs and the installer are built; it needs only the
standard library (`uv` is `winget install astral-sh.uv`, and plain `python` works as well).

Signing is on when the `SOYEHT_WINDOWS_SIGN_*` variables name a certificate (`signing_from`): every
program and the installer are signed with `signtool` and a timestamp. With none the packages are built
unsigned, with a loud message, and `--require-signature` turns that into an error. A certificate, a
password or a token never enters this repository.
"""

import argparse
import os
import shutil
import struct
import sys
from dataclasses import dataclass
from pathlib import Path

from generated_profile import PROFILES
from legacy_guard import require_next
from package_common import (
    HOMEPAGE,
    PROFILE_NAME,
    ROOT,
    PackagingError,
    absolute,
    add_build_arguments,
    built_programs,
    cargo_build,
    host_arch,
    make_plan,
    run,
    stream,
)
from package_common import Plan as BasePlan

PACKAGING = ROOT / "apps/desktop/packaging/windows"
SCRIPT = PACKAGING / "soyeht.iss"
PROFILE = PROFILES[PROFILE_NAME]
# What people see, and what the machine knows the installer by, all from the profile (ADR 0112).
# The name of the installer's entries (the shortcut, Add or remove programs, the description of a
# signature) is the window's title; the folder below Programs is the name of the app's folder on
# the Mac, which carries "next"; the installer id, and with it the key of Add or remove
# programs, is the profile's application id for Windows.
PRODUCT_NAME = PROFILE["displayNames"]["window"]
PUBLISHER = PRODUCT_NAME
INSTALL_FOLDER = PROFILE["displayNames"]["macApp"]
APP_ID = PROFILE["appIds"]["windows"]

ENGINE_PROGRAMS = ("soyeht", "soyeht-engine", "soyeht-ptyd", "soyeht-mcp", "soyeht-service-host")
DESKTOP_PROGRAMS = ("soyeht-desktop",)
# The PE subsystem of each program: the window and the service host open no console (GUI, 2), the
# others are console programs (3) that the window and the host start with a hidden console.
GUI, CONSOLE = 2, 3
SUBSYSTEMS = {
    "soyeht": CONSOLE,
    "soyeht-engine": CONSOLE,
    "soyeht-ptyd": CONSOLE,
    "soyeht-mcp": CONSOLE,
    "soyeht-service-host": GUI,
    "soyeht-desktop": GUI,
}
# Files beside the programs, which the installer puts in the install folder.
LICENSE_FILES = ("LICENSE", "NOTICE")

# architecture -> (rust target, PE machine, Inno architecture, folder of the Windows SDK tools).
# An x64 installer also opens on Windows on Arm, which runs x64 programs; an arm64 one does not
# open on x64.
ARCHITECTURES = {
    "x86_64": ("x86_64-pc-windows-msvc", 0x8664, "x64compatible", "x64"),
    "aarch64": ("aarch64-pc-windows-msvc", 0xAA64, "arm64", "arm64"),
}

TIMESTAMP_THUMBPRINT = "http://timestamp.digicert.com"
TIMESTAMP_AZURE = "http://timestamp.acs.microsoft.com"
SIGN_THUMBPRINT = "SOYEHT_WINDOWS_SIGN_THUMBPRINT"
SIGN_AZURE_DLIB = "SOYEHT_WINDOWS_SIGN_AZURE_DLIB"
SIGN_AZURE_METADATA = "SOYEHT_WINDOWS_SIGN_AZURE_METADATA"
SIGN_TIMESTAMP = "SOYEHT_WINDOWS_SIGN_TIMESTAMP_URL"
SIGNTOOL_PATH = "SOYEHT_WINDOWS_SIGNTOOL"
INNO_PATH = "SOYEHT_INNO_COMPILER"
SIGN_NAME = "soyehtsign"  # the name Inno knows the signing command by


@dataclass(frozen=True)
class Plan(BasePlan):
    @property
    def target(self):
        return ARCHITECTURES[self.arch][0]

    @property
    def machine(self):
        return ARCHITECTURES[self.arch][1]

    @property
    def allowed(self):
        return ARCHITECTURES[self.arch][2]

    @property
    def display_version(self):
        """`0.1.0`, and `0.1.0.N` for the Nth upload of one version, which Windows sorts after it."""
        return self.version if self.revision == 0 else f"{self.version}.{self.revision}"

    @property
    def file_version(self):
        """Four numbers, as the version resource of Windows wants them (`soyeht.rc` does the same)."""
        return f"{self.version}.{self.revision}"

    def artifact(self, out):
        return self.artifact_path(out, "windows", "exe")

    def stage(self, out):
        return Path(out) / f".stage-windows-{self.arch}"


# ---- reading the programs --------------------------------------------------------------------


def pe_header(data):
    """(machine, subsystem) of a Windows program, from its PE header."""
    if data[:2] != b"MZ" or len(data) < 0x40:
        raise PackagingError("not a Windows program (no MZ header)")
    (offset,) = struct.unpack_from("<I", data, 0x3C)
    if data[offset : offset + 4] != b"PE\0\0" or len(data) < offset + 24 + 70:
        raise PackagingError("not a Windows program (no PE header)")
    (machine,) = struct.unpack_from("<H", data, offset + 4)
    # The optional header starts 24 bytes in; the subsystem is 68 bytes into it in both formats.
    (subsystem,) = struct.unpack_from("<H", data, offset + 24 + 68)
    return machine, subsystem


def check_programs(programs, plan):
    """Stops a build for the wrong processor, or a window that would open a console."""
    for name, path in programs.items():
        machine, subsystem = pe_header(Path(path).read_bytes())
        if machine != plan.machine:
            raise PackagingError(f"{name}.exe is for machine 0x{machine:04X}, not {plan.arch} (0x{plan.machine:04X})")
        if subsystem != SUBSYSTEMS[name]:
            raise PackagingError(f"{name}.exe has PE subsystem {subsystem}, wanted {SUBSYSTEMS[name]}")


def check_names():
    """Every machine identifier of the rewrite carries "next", and none is the legacy app's (ADR 0112)."""
    for what, value in (("installer id", APP_ID), ("install folder", INSTALL_FOLDER)):
        require_next(what, value)


# ---- signing ---------------------------------------------------------------------------------


@dataclass(frozen=True)
class Signing:
    """How `signtool sign` is told which certificate to use, and when to stamp the time."""

    selection: tuple  # the arguments that pick the certificate or the signing service
    timestamp: str

    def command(self, signtool, path=None):
        """The `signtool sign` command line; `path` None leaves the file for the caller to add."""
        arguments = [str(signtool), "sign", "/fd", "SHA256", "/tr", self.timestamp, "/td", "SHA256"]
        arguments += ["/d", PRODUCT_NAME]
        return arguments + list(self.selection) + ([str(path)] if path else [])


def signing_from(environment):
    """The signing the `SOYEHT_WINDOWS_SIGN_*` variables ask for, or None when they ask for none.

    A certificate in the store of the person who builds is named by its SHA-1 thumbprint
    (`SOYEHT_WINDOWS_SIGN_THUMBPRINT`). Azure Artifact Signing is the signtool plug-in
    (`SOYEHT_WINDOWS_SIGN_AZURE_DLIB`) and the metadata file that names the account and the
    certificate profile (`SOYEHT_WINDOWS_SIGN_AZURE_METADATA`); the credentials the plug-in uses are
    the Azure ones of the environment. `SOYEHT_WINDOWS_SIGN_TIMESTAMP_URL` replaces the timestamp
    service of either."""
    thumbprint = environment.get(SIGN_THUMBPRINT, "").strip()
    dlib = environment.get(SIGN_AZURE_DLIB, "").strip()
    metadata = environment.get(SIGN_AZURE_METADATA, "").strip()
    if thumbprint and (dlib or metadata):
        raise PackagingError(f"{SIGN_THUMBPRINT} and the Azure variables both name a certificate: set one kind")
    if thumbprint:
        stamp = environment.get(SIGN_TIMESTAMP, "").strip() or TIMESTAMP_THUMBPRINT
        return Signing(("/sha1", thumbprint), stamp)
    if dlib or metadata:
        if not (dlib and metadata):
            raise PackagingError(f"Azure signing needs both {SIGN_AZURE_DLIB} and {SIGN_AZURE_METADATA}")
        stamp = environment.get(SIGN_TIMESTAMP, "").strip() or TIMESTAMP_AZURE
        return Signing(("/dlib", dlib, "/dmdf", metadata), stamp)
    return None


def inno_quote(argument):
    """An argument of the signing command as Inno's `/S` option reads it: `$q` is a quote, `$$` a dollar."""
    text = argument.replace("$", "$$")
    return f"$q{text}$q" if " " in text else text


def inno_sign_option(signing, signtool):
    """`/Ssoyehtsign=...`: Inno signs the installer and its uninstaller with this command (`$f` is the file)."""
    command = " ".join(inno_quote(part) for part in signing.command(signtool)) + " $f"
    return f"/S{SIGN_NAME}={command}"


def find_signtool(given, environment, host):
    """`signtool.exe` from `--signtool`, the environment, or the newest Windows SDK installed."""
    wanted = given or environment.get(SIGNTOOL_PATH)
    if wanted:
        return Path(wanted)
    kits = Path(environment.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "Windows Kits/10/bin"
    folders = [ARCHITECTURES[host][3], "x64"]
    for version in sorted(kits.glob("10.*"), reverse=True):
        for folder in folders:
            if (version / folder / "signtool.exe").is_file():
                return version / folder / "signtool.exe"
    raise PackagingError(f"signing is on but signtool.exe was not found: install the Windows SDK or set {SIGNTOOL_PATH}")


def check_build_tools(arch):
    """`ring` compiles C for Arm with clang, which Visual Studio does not install: LLVM must be on `PATH`."""
    if arch == "aarch64" and not shutil.which("clang"):
        raise PackagingError("an aarch64 build needs clang (LLVM) on PATH: winget install LLVM.LLVM")


def loud_unsigned():
    bar = "=" * 78
    return (
        f"{bar}\nUNSIGNED BUILD: none of the SOYEHT_WINDOWS_SIGN_* variables is set.\n"
        "Windows SmartScreen will warn a person who runs this installer, and a person may\n"
        "refuse it. Use it to test; do not publish it. docs/platforms.md, 'Windows packages'.\n"
        f"{bar}"
    )


def sign_file(signtool, signing, path):
    run(signing.command(signtool, path))
    verify(signtool, path)


def verify(signtool, path):
    run([signtool, "verify", "/pa", "/q", path])


# ---- the installer ---------------------------------------------------------------------------


def stage(plan, out):
    """The folder the installer packs: the programs and the licence files, all at its top."""
    staging = plan.stage(out)
    shutil.rmtree(staging, ignore_errors=True)
    staging.mkdir(parents=True)
    for name, source in plan.programs.items():
        shutil.copy2(source, staging / f"{name}.exe")
    for name in LICENSE_FILES:
        shutil.copy2(ROOT / name, staging / name)
    return staging


def inno_arguments(plan, staging, out, signing, signtool):
    """The `ISCC` command line after the compiler: every value `soyeht.iss` reads, then the script."""
    values = {
        "AppName": PRODUCT_NAME,
        "AppId": APP_ID,
        "InstallFolder": INSTALL_FOLDER,
        "Version": plan.display_version,
        "VersionInfo": plan.file_version,
        "Allowed": plan.allowed,
        "Stage": staging,
        "OutputDir": out,
        "OutputName": plan.artifact(out).stem,
        "Profile": PROFILE_NAME,
        "Publisher": PUBLISHER,
        "Url": HOMEPAGE,
        "Icon": PACKAGING / "soyeht.ico",
    }
    arguments = [f"/D{name}={value}" for name, value in values.items()]
    if signing:
        arguments += ["/DSigned=1", inno_sign_option(signing, signtool)]
    return ["/Qp", *arguments, str(SCRIPT)]


def find_inno(given, environment):
    wanted = given or environment.get(INNO_PATH)
    if wanted:
        return Path(wanted)
    found = shutil.which("ISCC")
    if found:
        return Path(found)
    for variable in ("LOCALAPPDATA", "ProgramFiles(x86)", "ProgramFiles"):
        base = environment.get(variable)
        subfolder = "Programs/Inno Setup 6" if variable == "LOCALAPPDATA" else "Inno Setup 6"
        if base and (Path(base) / subfolder / "ISCC.exe").is_file():
            return Path(base) / subfolder / "ISCC.exe"
    raise PackagingError(f"Inno Setup 6 was not found: winget install JRSoftware.InnoSetup, or set {INNO_PATH}")


def build_installer(plan, out, inno, signing, signtool):
    staging = stage(plan, out)
    if signing:
        for path in sorted(staging.glob("*.exe")):
            sign_file(signtool, signing, path)
    stream([inno, *inno_arguments(plan, staging, out, signing, signtool)])
    target = plan.artifact(out)
    if signing:
        verify(signtool, target)
    shutil.rmtree(staging)
    return target


# ---- building and the command line -----------------------------------------------------------


def parse(argv):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--arch", choices=[*sorted(ARCHITECTURES), "all"], default="all")
    add_build_arguments(parser)
    parser.add_argument("--inno", help=f"ISCC.exe of Inno Setup 6 (default: {INNO_PATH}, PATH, the usual folders)")
    parser.add_argument("--signtool", help=f"signtool.exe (default: {SIGNTOOL_PATH}, else the newest Windows SDK)")
    parser.add_argument("--require-signature", action="store_true", help="fail unless a certificate is named")
    return parser.parse_args(argv)


def main(argv=None):
    args = parse(argv)
    check_names()
    args.engine_target = absolute(args.engine_target)
    args.desktop_target = absolute(args.desktop_target)
    out = Path(args.out).resolve()
    arches = sorted(ARCHITECTURES) if args.arch == "all" else [args.arch]
    signing = signing_from(os.environ)
    if args.require_signature and not signing:
        raise PackagingError("--require-signature, but none of the SOYEHT_WINDOWS_SIGN_* variables is set")
    signtool = find_signtool(args.signtool, os.environ, host_arch()) if signing else None
    inno = find_inno(args.inno, os.environ)
    if not signing:
        print(loud_unsigned(), file=sys.stderr)
    out.mkdir(parents=True, exist_ok=True)
    written = []
    for arch in arches:
        triple = ARCHITECTURES[arch][0]
        if not args.skip_build:
            check_build_tools(arch)
            cargo_build(args.engine_target, args.desktop_target, triple)
        programs = built_programs(
            args.engine_target, args.desktop_target, ENGINE_PROGRAMS, DESKTOP_PROGRAMS, triple, ".exe"
        )
        plan = make_plan(Plan, arch, programs)
        check_programs(programs, plan)
        written.append(build_installer(plan, out, inno, signing, signtool))
    for path in written:
        print(f"{path} ({path.stat().st_size} bytes, {'signed' if signing else 'UNSIGNED'})")


if __name__ == "__main__":
    try:
        main()
    except PackagingError as failure:
        sys.exit(f"package_windows: {failure}")
