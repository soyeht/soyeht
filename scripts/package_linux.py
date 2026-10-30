#!/usr/bin/env python3
"""Build the Linux packages of the production profile: a `.deb` and an AppImage (ADR 0121).

    uv run python scripts/package_linux.py [--format deb|appimage|all] [--arch x86_64|aarch64]
        [--skip-build] [--engine-target DIR] [--desktop-target DIR] [--out DIR]
        [--runtime FILE] [--max-glibc VERSION|any]

It builds the release programs of both cargo workspaces for the host (a package of the other
architecture is built on that architecture), with `SOYEHT_BUILD_PROFILE=prod` so that the
programs find the production profile of `contracts/profiles.json` without setup (ADR 0112), and
writes `dist/soyeht-<version>-linux-<arch>.deb` and `.AppImage`. The version is
`contracts/release.json` and nothing else (ADR 0115).

A package is only as portable as the glibc of the machine that built it, so the script reads the
newest glibc symbol the programs need, writes it into the `.deb` as `libc6 (>= X)` and refuses
to package a build above `--max-glibc` (default: the glibc of Ubuntu 22.04). Build in that base
(`docs/platforms.md`, "Linux packages"). The `.deb` is written by this script alone, in Python,
so it needs no `dpkg-deb`; the AppImage needs `mksquashfs` and a type 2 runtime, which is pinned by
hash and downloaded once.
"""

import argparse
import hashlib
import io
import lzma
import os
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
import release  # noqa: E402  (the scripts directory is on the path of a script)
from generated_profile import BUILD_ENVIRONMENT, PROFILES  # noqa: E402
from package_common import PackagingError, sha256_of  # noqa: E402

PROFILE = "prod"
APPLICATION_ID = PROFILES[PROFILE]["appIds"]["linux"]
PACKAGE = "soyeht"
PACKAGING = ROOT / "apps/desktop/packaging/linux"
# Where the programs sit inside a package: one folder, as `soyeht service install --programs`
# and the window's own start of the engine expect (ADR 0102, ADR 0114).
LIBRARY_FOLDER = "usr/lib/soyeht"
# The command line and the window are on the path; the engine, the supervisor and the MCP
# server are not, since only the window and the engine start them.
ON_PATH = ("soyeht", "soyeht-desktop")
ENGINE_PROGRAMS = ("soyeht", "soyeht-engine", "soyeht-ptyd", "soyeht-mcp")
ENGINE_PACKAGES = ("soyeht-engine", "soyeht-ptyd", "soyeht-cli", "soyeht-mcp", "soyeht-service")
DESKTOP_PROGRAMS = ("soyeht-desktop",)

# architecture -> (Debian name, type 2 runtime sha256). The runtime is the one at
# github.com/AppImage/type2-runtime, release 20251108.
ARCHITECTURES = {
    "x86_64": ("amd64", "2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d"),
    "aarch64": ("arm64", "00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444"),
}
RUNTIME_URL = "https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-{arch}"
DEFAULT_MAX_GLIBC = "2.35"

MAINTAINER = "Caio Salgado <caio.salgado@hotmail.com>"
HOMEPAGE = "https://github.com/soyeht/soyeht"
SUMMARY = "Terminals and coding agents in one workspace"
DESCRIPTION = (
    " Soyeht puts terminals, coding agents, files and web pages side by side in one\n"
    " workspace. An engine and a terminal supervisor run in the background, so the\n"
    " terminals and agents keep running when the window closes.\n"
    " .\n"
    " The command line, soyeht, registers them to start at login (soyeht service install)\n"
    " and removes them again (soyeht uninstall)."
)

# The libraries a program is linked against, by name, and the Debian package that holds each. A
# name that is not here stops the build, so a new link-time dependency is declared by a person.
LINKED_PACKAGES = {
    "libc.so.6": "libc6",
    "libm.so.6": "libc6",
    "libdl.so.2": "libc6",
    "libpthread.so.0": "libc6",
    "librt.so.1": "libc6",
    "libutil.so.1": "libc6",
    "ld-linux-x86-64.so.2": "libc6",
    "ld-linux-aarch64.so.1": "libc6",
    "libgcc_s.so.1": "libgcc-s1",
    "libxcb.so.1": "libxcb1",
    "libxkbcommon.so.0": "libxkbcommon0",
    "libxkbcommon-x11.so.0": "libxkbcommon-x11-0",
}
# What the window loads when it runs (`dlopen`: the graphics stack, the Wayland client, and the
# web view of ADR 0110), which no linker records. It cannot open without the first group, so they
# are `Depends`; the rest serves the OpenGL fallback, a vendor driver that replaces Mesa, or the
# optional web pane, so they are `Recommends`.
LOADED_REQUIRED = ("libvulkan1", "libwayland-client0")
LOADED_RECOMMENDED = (
    "mesa-vulkan-drivers",
    "libegl1",
    "libwayland-egl1",
    "libgtk-3-0t64 | libgtk-3-0",
    "libwebkit2gtk-4.1-0",
)

ICON_SIZES = (16, 32, 64, 128, 256, 512)


@dataclass(frozen=True)
class Plan:
    """Everything a package depends on, so that two runs of one commit write the same bytes."""

    version: str
    revision: int
    arch: str
    epoch: int
    programs: dict  # program name -> built file

    @property
    def deb_arch(self):
        return ARCHITECTURES[self.arch][0]

    @property
    def deb_version(self):
        """`0.1.0`, and `0.1.0-N` for the Nth upload of one version, which sorts after it."""
        return self.version if self.revision == 0 else f"{self.version}-{self.revision}"

    def artifact(self, out, extension):
        return out / f"{PACKAGE}-{self.version}-linux-{self.arch}.{extension}"


# ---- reading the programs --------------------------------------------------------------------


def glibc_floor(data):
    """The newest glibc version a program needs, as a tuple, from the version names in its
    dynamic string table; `()` for a program that needs none."""
    found = {tuple(map(int, m.group(1).decode().split("."))) for m in re.finditer(rb"GLIBC_(\d+(?:\.\d+)+)", data)}
    return max(found, default=())


def linked_libraries(readelf_output):
    """The sonames in the `(NEEDED)` lines of `readelf -d`."""
    return re.findall(r"\(NEEDED\)\s+Shared library: \[([^\]]+)\]", readelf_output)


def inspect_programs(programs):
    """(the glibc floor of all programs, the sonames they link). Fails on a soname nobody declared."""
    floor, sonames = (), set()
    for path in programs.values():
        floor = max(floor, glibc_floor(Path(path).read_bytes()))
        shown = subprocess.run(["readelf", "-d", str(path)], capture_output=True, text=True, check=True)
        sonames |= set(linked_libraries(shown.stdout))
    unknown = sorted(sonames - LINKED_PACKAGES.keys())
    if unknown:
        raise PackagingError(
            f"the programs link {', '.join(unknown)}; add the package that holds each to "
            "LINKED_PACKAGES (and to the AppImage notes in docs/platforms.md)"
        )
    return floor, sonames


def version_text(version):
    return ".".join(map(str, version))


def check_glibc(floor, limit):
    if limit != "any" and floor > tuple(map(int, limit.split("."))):
        raise PackagingError(
            f"the programs need glibc {version_text(floor)}, above {limit}: build in the "
            "Ubuntu 22.04 base (docs/platforms.md, 'Linux packages'), or pass --max-glibc any"
        )


# ---- the files of a package ------------------------------------------------------------------


def write_file(path, data, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    path.chmod(mode)


def normalize_modes(root):
    """Every staged folder 0755, whatever the builder's umask: the AppImage keeps what the
    staging folder says, and a person's umask must not decide who can read the programs."""
    root.chmod(0o755)
    for path in root.rglob("*"):
        if path.is_dir() and not path.is_symlink():
            path.chmod(0o755)


def metainfo_text(template, plan):
    date = datetime.fromtimestamp(plan.epoch, timezone.utc).strftime("%Y-%m-%d")
    line = f'<release version="{plan.version}" date="{date}"/>'
    if "<!--RELEASES-->" not in template:
        raise PackagingError("the metainfo template lost its <!--RELEASES--> marker")
    return template.replace("<!--RELEASES-->", line)


def stage_shared(root, plan):
    """The files both formats install below `usr`: programs, launcher entry, icons, metainfo."""
    for name, source in plan.programs.items():
        write_file(root / LIBRARY_FOLDER / name, Path(source).read_bytes(), 0o755)
    entry = (PACKAGING / f"{APPLICATION_ID}.desktop").read_bytes()
    write_file(root / f"usr/share/applications/{APPLICATION_ID}.desktop", entry)
    for size in ICON_SIZES:
        icon = PACKAGING / f"icons/hicolor/{size}x{size}/apps/soyeht.png"
        write_file(root / f"usr/share/icons/hicolor/{size}x{size}/apps/soyeht.png", icon.read_bytes())
    template = (PACKAGING / f"{APPLICATION_ID}.metainfo.xml").read_text()
    text = metainfo_text(template, plan)
    write_file(root / f"usr/share/metainfo/{APPLICATION_ID}.metainfo.xml", text.encode())


def copyright_text():
    return (
        "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n"
        f"Upstream-Name: Soyeht\nSource: {HOMEPAGE}\n\n"
        "Files: *\nCopyright: 2026 Caio Salgado\nLicense: Apache-2.0\n"
        " On Debian systems the full text of the Apache License 2.0 is in\n"
        " /usr/share/common-licenses/Apache-2.0.\n"
    )


def stage_deb(root, plan):
    stage_shared(root, plan)
    for name in ON_PATH:
        link = root / "usr/bin" / name
        link.parent.mkdir(parents=True, exist_ok=True)
        link.symlink_to(f"../lib/soyeht/{name}")
    documents = root / "usr/share/doc" / PACKAGE
    write_file(documents / "copyright", copyright_text().encode())
    write_file(documents / "NOTICE", (ROOT / "NOTICE").read_bytes())
    normalize_modes(root)


def build_id(plan):
    """What the AppImage's launcher compares to know whether its copied programs are this build's."""
    digest = hashlib.sha256()
    for name in sorted(plan.programs):
        digest.update(name.encode() + Path(plan.programs[name]).read_bytes())
    return f"{plan.version}+{digest.hexdigest()[:16]}\n"


def stage_appimage(root, plan):
    stage_shared(root, plan)
    write_file(root / LIBRARY_FOLDER / "BUILD_ID", build_id(plan).encode())
    write_file(root / "AppRun", (PACKAGING / "AppRun").read_bytes(), 0o755)
    entry = (PACKAGING / f"{APPLICATION_ID}.desktop").read_bytes()
    write_file(root / f"{APPLICATION_ID}.desktop", entry)
    write_file(root / "soyeht.png", (PACKAGING / "icons/hicolor/256x256/apps/soyeht.png").read_bytes())
    (root / ".DirIcon").symlink_to("soyeht.png")
    normalize_modes(root)


# ---- the .deb --------------------------------------------------------------------------------


def dependencies(floor, sonames):
    """(Depends, Recommends) lines, from what the programs link and what the window loads."""
    linked = {LINKED_PACKAGES[name] for name in sonames} - {"libc6"}
    depends = [f"libc6 (>= {version_text(floor)})"] if floor else ["libc6"]
    depends += sorted(linked) + list(LOADED_REQUIRED)
    return ", ".join(depends), ", ".join(LOADED_RECOMMENDED)


def control_text(plan, floor, sonames, size):
    depends, recommends = dependencies(floor, sonames)
    return (
        f"Package: {PACKAGE}\nVersion: {plan.deb_version}\nArchitecture: {plan.deb_arch}\n"
        f"Maintainer: {MAINTAINER}\nInstalled-Size: {size}\nDepends: {depends}\n"
        f"Recommends: {recommends}\nSection: devel\nPriority: optional\nHomepage: {HOMEPAGE}\n"
        f"Description: {SUMMARY}\n{DESCRIPTION}\n"
    )


def tar_entry(name, plan, kind, mode, size=0, link=""):
    info = tarfile.TarInfo(name)
    info.type, info.mode, info.size, info.linkname = kind, mode, size, link
    info.uid = info.gid = 0
    info.uname = info.gname = "root"
    info.mtime = plan.epoch
    return info


def deb_tar(files, plan):
    """An xz-compressed tar of `files` ((name, mode, bytes), a link as bytes=None, link=target)
    with the owner root and every time the build's, in sorted order."""
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w", format=tarfile.GNU_FORMAT) as archive:
        for name, kind, mode, data, link in sorted(files, key=lambda item: item[0]):
            archive.addfile(tar_entry(name, plan, kind, mode, len(data or b""), link), io.BytesIO(data or b""))
    return lzma.compress(buffer.getvalue(), preset=9 | lzma.PRESET_EXTREME)


def walk(root):
    """The staged files of `root` as tar entries named `./path`, with every parent folder."""
    entries = [("./", tarfile.DIRTYPE, 0o755, None, "")]
    for path in sorted(root.rglob("*")):
        name = "./" + path.relative_to(root).as_posix()
        if path.is_symlink():
            entries.append((name, tarfile.SYMTYPE, 0o777, None, os.readlink(path)))
        elif path.is_dir():
            entries.append((name + "/", tarfile.DIRTYPE, 0o755, None, ""))
        else:
            mode = 0o755 if path.stat().st_mode & 0o111 else 0o644
            entries.append((name, tarfile.REGTYPE, mode, path.read_bytes(), ""))
    return entries


def md5sums(entries):
    return "".join(
        f"{hashlib.md5(data).hexdigest()}  {name[2:]}\n"
        for name, kind, _, data, _ in entries
        if kind == tarfile.REGTYPE
    )


def ar_member(name, data, epoch):
    header = f"{name:<16}{epoch:<12}{0:<6}{0:<6}{'100644':<8}{len(data):<10}`\n"
    return header.encode() + data + (b"\n" if len(data) % 2 else b"")


def ar_archive(members, epoch):
    return b"!<arch>\n" + b"".join(ar_member(name, data, epoch) for name, data in members)


def build_deb(plan, floor, sonames, out):
    staging = out / f".stage-deb-{plan.arch}"
    shutil.rmtree(staging, ignore_errors=True)
    stage_deb(staging, plan)
    data_entries = walk(staging)
    shutil.rmtree(staging)
    size = sum(len(entry[3] or b"") for entry in data_entries) // 1024 + 1
    control = [
        ("./control", tarfile.REGTYPE, 0o644, control_text(plan, floor, sonames, size).encode(), ""),
        ("./md5sums", tarfile.REGTYPE, 0o644, md5sums(data_entries).encode(), ""),
    ]
    scripts = {"postinst": "refresh-caches", "postrm": "refresh-caches", "prerm": "prerm"}
    for name, source in scripts.items():
        control.append((f"./{name}", tarfile.REGTYPE, 0o755, (PACKAGING / "deb" / source).read_bytes(), ""))
    members = [
        ("debian-binary", b"2.0\n"),
        ("control.tar.xz", deb_tar(control, plan)),
        ("data.tar.xz", deb_tar(data_entries, plan)),
    ]
    target = plan.artifact(out, "deb")
    target.write_bytes(ar_archive(members, plan.epoch))
    return target


# ---- the AppImage ----------------------------------------------------------------------------


def runtime_file(arch, given, cache):
    """The type 2 runtime for `arch`: the file given, else the pinned download. A download is
    checked by hash before it is renamed into `cache`, so an unverified file is never kept."""
    wanted = ARCHITECTURES[arch][1]
    path = Path(given) if given else cache / f"runtime-{arch}"
    if not path.exists():
        cache.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=cache, prefix=".download-", delete=False) as partial:
            with urllib.request.urlopen(RUNTIME_URL.format(arch=arch), timeout=60) as response:
                partial.write(response.read())
        if sha256_of(partial.name) != wanted:
            Path(partial.name).unlink()
            raise PackagingError(f"the download of the {arch} runtime is not the pinned one (sha256 {wanted})")
        os.replace(partial.name, path)
    found = sha256_of(path)
    if found != wanted:
        raise PackagingError(f"{path} is not the pinned runtime (sha256 {found}, wanted {wanted})")
    return path


def build_appimage(plan, runtime, out):
    staging = out / f".stage-appimage-{plan.arch}"
    shutil.rmtree(staging, ignore_errors=True)
    stage_appimage(staging, plan)
    image = out / f".stage-appimage-{plan.arch}.squashfs"
    image.unlink(missing_ok=True)
    environment = dict(os.environ, SOURCE_DATE_EPOCH=str(plan.epoch))
    subprocess.run(
        ["mksquashfs", str(staging), str(image), "-root-owned", "-noappend", "-quiet",
         "-comp", "zstd", "-Xcompression-level", "19", "-b", "1M"],
        check=True, env=environment,
    )  # fmt: skip
    target = plan.artifact(out, "AppImage")
    target.write_bytes(Path(runtime).read_bytes() + image.read_bytes())
    target.chmod(0o755)
    image.unlink()
    shutil.rmtree(staging)
    return target


# ---- building and the command line -----------------------------------------------------------


def host_arch():
    machine = platform.machine()
    return {"arm64": "aarch64", "amd64": "x86_64"}.get(machine, machine)


def cargo_build(engine_target, desktop_target):
    """Release programs of both workspaces for the production profile; `--locked`, default features."""
    environment = dict(os.environ, **{BUILD_ENVIRONMENT: PROFILE})
    for folder, target, extra in (
        ("engine", engine_target, [arg for name in ENGINE_PACKAGES for arg in ("-p", name)]),
        ("apps/desktop", desktop_target, []),
    ):
        env = dict(environment, CARGO_TARGET_DIR=str(target)) if target else environment
        subprocess.run(["cargo", "build", "--release", "--locked", *extra], cwd=ROOT / folder, env=env, check=True)


def built_programs(engine_target, desktop_target):
    found = {}
    for names, target, folder in (
        (ENGINE_PROGRAMS, engine_target, "engine"),
        (DESKTOP_PROGRAMS, desktop_target, "apps/desktop"),
    ):
        release_folder = Path(target or ROOT / folder / "target") / "release"
        for name in names:
            if not (release_folder / name).is_file():
                raise PackagingError(f"{release_folder / name} is missing: build first, or drop --skip-build")
            found[name] = release_folder / name
    return found


def source_epoch():
    """`SOURCE_DATE_EPOCH`, else the time of the commit, so a rebuild of one commit is identical."""
    if os.environ.get("SOURCE_DATE_EPOCH"):
        return int(os.environ["SOURCE_DATE_EPOCH"])
    shown = subprocess.run(["git", "log", "-1", "--format=%ct"], cwd=ROOT, capture_output=True, text=True)
    return int(shown.stdout.strip()) if shown.returncode == 0 and shown.stdout.strip() else 0


def absolute(path):
    return str(Path(path).resolve()) if path else None


def parse(argv):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--format", choices=("deb", "appimage", "all"), default="all")
    parser.add_argument("--arch", choices=sorted(ARCHITECTURES), default=host_arch())
    parser.add_argument("--skip-build", action="store_true", help="package the programs already built")
    parser.add_argument("--engine-target", help="cargo target directory of engine/ (default engine/target)")
    parser.add_argument("--desktop-target", help="cargo target directory of apps/desktop/")
    parser.add_argument("--out", default=str(ROOT / "dist"))
    parser.add_argument("--cache", help="where the downloaded runtime is kept (default OUT/.cache)")
    parser.add_argument("--runtime", help="the type 2 runtime file (default: download, pinned by hash)")
    parser.add_argument("--max-glibc", default=DEFAULT_MAX_GLIBC, help="newest glibc allowed, or 'any'")
    return parser.parse_args(argv)


def main(argv=None):
    args = parse(argv)
    # Both are used from other working folders (cargo runs in each workspace).
    args.engine_target = absolute(args.engine_target)
    args.desktop_target = absolute(args.desktop_target)
    out = Path(args.out).resolve()
    if args.arch != host_arch() and not args.skip_build:
        raise PackagingError(f"this machine is {host_arch()}: build {args.arch} packages on one")
    if not args.skip_build:
        cargo_build(args.engine_target, args.desktop_target)
    programs = built_programs(args.engine_target, args.desktop_target)
    floor, sonames = inspect_programs(programs)
    check_glibc(floor, args.max_glibc)
    made = release.current(ROOT)
    plan = Plan(str(made.version), made.revision, args.arch, source_epoch(), programs)
    out.mkdir(parents=True, exist_ok=True)
    written = []
    if args.format in ("deb", "all"):
        written.append(build_deb(plan, floor, sonames, out))
    if args.format in ("appimage", "all"):
        runtime = runtime_file(args.arch, args.runtime, Path(args.cache) if args.cache else out / ".cache")
        written.append(build_appimage(plan, runtime, out))
    for path in written:
        print(f"{path} ({path.stat().st_size} bytes, needs glibc {version_text(floor) or 'none'})")


if __name__ == "__main__":
    try:
        main()
    except (PackagingError, subprocess.CalledProcessError) as failure:
        sys.exit(f"package_linux: {failure}")
