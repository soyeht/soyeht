"""What the packaging scripts of every platform share (`package_mac.py`, `package_linux.py`,
`package_windows.py`; ADRs 0120, 0121 and 0123): the error a step raises, the digest of a file, the
command runners, the product version and the name of a package file (`contracts/release.json`, ADR
0115), the release build of the two cargo workspaces and the programs it leaves."""

from __future__ import annotations

import hashlib
import os
import platform
import subprocess
from dataclasses import dataclass
from pathlib import Path

import release
from generated_profile import BUILD_ENVIRONMENT

ROOT = Path(__file__).resolve().parents[1]
COMMAND_SECONDS = 600
# The production profile, by its name in `contracts/profiles.json` (a package ships no other).
PROFILE_NAME = "prod"
PACKAGE = "soyeht"
HOMEPAGE = "https://github.com/soyeht/soyeht"
# The programs of `engine/` that a package ships, by cargo package.
ENGINE_PACKAGES = ("soyeht-engine", "soyeht-ptyd", "soyeht-cli", "soyeht-mcp", "soyeht-service")


class PackagingError(Exception):
    """A step failed; the message says which and why."""


def sha256_of(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_checksum(path):
    """`<file>.sha256` beside `path`, in the format `shasum -a 256 -c` reads."""
    path = Path(path)
    checksum = path.with_name(path.name + ".sha256")
    checksum.write_text(f"{sha256_of(path)}  {path.name}\n")
    return checksum


def _words(command):
    return [str(part) for part in command]


def run(command, *, check=True, timeout=COMMAND_SECONDS, environment=None, cwd=None):
    """Run a command, print it first, and return the finished process (output captured)."""
    print("+ " + " ".join(_words(command)), flush=True)
    result = subprocess.run(
        _words(command), capture_output=True, text=True, timeout=timeout, env=environment, cwd=cwd,
    )
    if check and result.returncode:
        detail = (result.stderr or result.stdout).strip()[-2000:]
        raise PackagingError(f"{command[0]} failed with status {result.returncode}:\n{detail}")
    return result


def stream(command, *, environment=None, cwd=None):
    """Run a long build with its output on the terminal."""
    print("+ " + " ".join(_words(command)), flush=True)
    status = subprocess.run(_words(command), env=environment, cwd=cwd).returncode
    if status:
        raise PackagingError(f"{command[0]} failed with status {status}")


@dataclass(frozen=True)
class Plan:
    """Everything a package depends on, so that two runs of one commit write the same bytes."""

    version: str
    revision: int
    arch: str
    epoch: int
    programs: dict  # program name -> built file

    def artifact_path(self, out, system, extension):
        """`soyeht-<version>-<system>-<arch>.<extension>`: the name the update manifest expects."""
        return artifact_path(out, self.version, system, self.arch, extension)


def artifact_path(out, version, system, arch, extension):
    return Path(out) / f"{PACKAGE}-{version}-{system}-{arch}.{extension}"


def make_plan(plan_class, arch, programs):
    """The plan of this commit's release: the version and revision come from the contract only."""
    made = release.current(ROOT)
    return plan_class(str(made.version), made.revision, arch, source_epoch(), programs)


def host_arch():
    machine = platform.machine().lower()
    return {"arm64": "aarch64", "amd64": "x86_64"}.get(machine, machine)


def source_epoch():
    """`SOURCE_DATE_EPOCH`, else the time of the commit, so a rebuild of one commit is identical."""
    if os.environ.get("SOURCE_DATE_EPOCH"):
        return int(os.environ["SOURCE_DATE_EPOCH"])
    shown = subprocess.run(["git", "log", "-1", "--format=%ct"], cwd=ROOT, capture_output=True, text=True)
    return int(shown.stdout.strip()) if shown.returncode == 0 and shown.stdout.strip() else 0


def absolute(path):
    return str(Path(path).resolve()) if path else None


def add_build_arguments(parser):
    """The options every package script has for building and for where the result goes."""
    parser.add_argument("--skip-build", action="store_true", help="package the programs already built")
    parser.add_argument("--engine-target", help="cargo target directory of engine/ (default engine/target)")
    parser.add_argument("--desktop-target", help="cargo target directory of apps/desktop/")
    parser.add_argument("--out", default=str(ROOT / "dist"))


# What the paths of the machine that built a release are called inside it.
REMAPPED_SOURCE = "/soyeht"
REMAPPED_CARGO_HOME = "/cargo"
REMAPPED_RUSTUP_HOME = "/rustup"
REMAPPED_TARGET = "/target"
REMAPPED_HOME = "/home"


def builder_paths(environment):
    """(path, name) of every place of the builder's machine a build can write into a program, the
    widest first: rustc maps a path by the last rule that matches it, so the folder of the source
    and the cargo folders, which sit below the home folder, come after it."""
    home = Path.home()
    cargo_home = Path(environment.get("CARGO_HOME") or home / ".cargo")
    rustup_home = Path(environment.get("RUSTUP_HOME") or home / ".rustup")
    paths = [(home, REMAPPED_HOME), (cargo_home, REMAPPED_CARGO_HOME), (rustup_home, REMAPPED_RUSTUP_HOME)]
    if environment.get("CARGO_TARGET_DIR"):
        paths.append((Path(environment["CARGO_TARGET_DIR"]).resolve(), REMAPPED_TARGET))
    paths.append((ROOT, REMAPPED_SOURCE))
    return paths


def release_environment(environment=None):
    """The environment of a release build: the production profile built in (ADR 0112), and no
    path of the builder's machine in a program (a panic message, a debug record or a library
    path names its file). `CARGO_ENCODED_RUSTFLAGS` carries the flags, since a path with a blank,
    a user name with a space, would be split in two by `RUSTFLAGS`; flags already set are kept."""
    result = dict(os.environ if environment is None else environment)
    if result.get("CARGO_ENCODED_RUSTFLAGS"):
        flags = result["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
    else:
        flags = result.get("RUSTFLAGS", "").split()
    flags += [f"--remap-path-prefix={path}={name}" for path, name in builder_paths(result)]
    result["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    result.pop("RUSTFLAGS", None)
    result[BUILD_ENVIRONMENT] = PROFILE_NAME
    return result


def cargo_build(engine_target, desktop_target, triple=None):
    """Release programs of both workspaces for the production profile; `--locked`, default
    features. `triple` builds for another target of the same system (Windows builds arm64 and x64
    on either). The output is on the terminal."""
    target = ["--target", triple] if triple else []
    for folder, directory, extra in (
        ("engine", engine_target, [arg for name in ENGINE_PACKAGES for arg in ("-p", name)]),
        ("apps/desktop", desktop_target, []),
    ):
        base = dict(os.environ)
        if directory:
            base["CARGO_TARGET_DIR"] = str(directory)
        environment = release_environment(base)
        stream(["cargo", "build", "--release", "--locked", *target, *extra], environment=environment, cwd=ROOT / folder)


def built_programs(engine_target, desktop_target, engine_programs, desktop_programs, triple=None, suffix=""):
    """{program: file} of the release builds; a missing one stops the package."""
    found = {}
    for names, directory, folder in (
        (engine_programs, engine_target, "engine"),
        (desktop_programs, desktop_target, "apps/desktop"),
    ):
        release_folder = Path(directory or ROOT / folder / "target")
        release_folder = release_folder / triple / "release" if triple else release_folder / "release"
        for name in names:
            path = release_folder / (name + suffix)
            if not path.is_file():
                raise PackagingError(f"{path} is missing: build first, or drop --skip-build")
            found[name] = path
    return found
