"""What the packaging scripts of every platform share (`package_mac.py`, `package_linux.py`,
`package_windows.py`; ADRs 0120, 0121 and 0123): the error a step raises, the digest of a file, the
command runners, the product version and the name of a package file (`contracts/release.json`, ADR
0115), the license documents every package carries, the release build of the two cargo workspaces and the programs it leaves."""

from __future__ import annotations

import hashlib
import json
import os
import platform
import shlex
import subprocess
import sys
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
# The license documents every package ships: the product's license and notice, and the generated
# notices of its third-party components (`scripts/third_party_notices.py`, ADR 0149).
LICENSE_DOCUMENTS = ("LICENSE", "NOTICE", "THIRD-PARTY-NOTICES.txt")
# The biggest a package may be, by `<system>-<arch>.<extension>`, in bytes (`package_sizes.json`).
SIZE_CEILINGS = Path(__file__).with_name("package_sizes.json")


class PackagingError(Exception):
    """A step failed; the message says which and why."""


def license_document(name):
    """The bytes of one of `LICENSE_DOCUMENTS`; a package without it is not a release."""
    path = ROOT / name
    if not path.is_file():
        raise PackagingError(f"{name} is missing: uv run python scripts/third_party_notices.py generate")
    return path.read_bytes()


def readable_by_others(path):
    """Whether group or others have any access to `path`: a private key (a notary `.p8`, the update
    signing seed) must not be. Raises `OSError` when the file cannot be inspected."""
    return bool(Path(path).stat().st_mode & 0o077)


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


def size_ceiling(path, ceilings):
    """`(kind, ceiling)` of a finished package: the kind is the key of `package_sizes.json` that
    ends the file name (`soyeht-<version>-<system>-<arch>.<extension>`, the version may have
    hyphens), the ceiling `None` for a kind that is not checked yet. A name that no key ends is
    an error, not a package to skip: a typo here would switch the check off."""
    name = Path(path).name
    head = PACKAGE + "-"
    kinds = [kind for kind in ceilings if name.startswith(head) and name.endswith("-" + kind)]
    if not kinds:
        raise PackagingError(f"{name} is not a package kind of scripts/package_sizes.json: add its line")
    kind = max(kinds, key=len)
    return kind, ceilings[kind]


def size_problem(path, ceilings=None, reserve=0):
    """`(problem, warning)` for a finished package, each a message or None. The problem is a size
    over the ceiling recorded for it: a package grows past its ceiling only on purpose, so raise
    the number in `package_sizes.json` in the same change and say why. The warning is a size
    more than 10% under it, which asks for the ceiling to come down but never stops a release.
    A kind whose ceiling is `null` (a system this host cannot build) is not checked. `reserve` is
    what the file will still gain (a signature, a notarization ticket), so a package can be checked
    before that work is paid for."""
    ceilings = json.loads(SIZE_CEILINGS.read_text())["ceilings"] if ceilings is None else ceilings
    _, ceiling = size_ceiling(path, ceilings)
    size = Path(path).stat().st_size + reserve
    name = Path(path).name
    if ceiling is None:
        return None, None
    if size > ceiling:
        will = f" ({reserve} of them reserved for what is still to be added)" if reserve else ""
        return f"{name} is {size} bytes{will}, over its ceiling of {ceiling} (scripts/package_sizes.json)", None
    if size < ceiling * 0.9:
        return None, f"{name} is {size} bytes, 10% under its ceiling of {ceiling}: lower it in scripts/package_sizes.json"
    return None, None


def check_sizes(paths, reserve=0, warn=True):
    """Stops the script when a finished package is over its ceiling or has no kind in the file,
    and prints the warning of one far under it. Every packaging script ends with this; a script
    whose last steps cost something (notarization) also runs it before them, with a `reserve` and
    `warn=False` (the last check prints the warning once)."""
    results = [size_problem(path, reserve=reserve) for path in paths]
    for _, warning in results:
        if warning and warn:
            print(f"size: {warning}", file=sys.stderr)
    if problems := [problem for problem, _ in results if problem]:
        raise PackagingError("; ".join(problems))


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


def is_windows():
    return os.name == "nt"


def c_flags(paths):
    """The flags that keep a builder's path out of `__FILE__` and the debug records of the C
    libraries a program is linked with (the SQLite and tree-sitter of the engine, built by the
    `cc` crate). Only clang and gcc have them (`-ffile-prefix-map`), so a Windows release is
    compiled with clang as well (see `release_environment`)."""
    return [f"-ffile-prefix-map={path}={name}" for path, name in paths]


def quoted_flag(flag, windows):
    """`flag` as `cc` reads it from `CFLAGS` with `CC_SHELL_ESCAPED_FLAGS`: quoted when it holds a
    blank, a quote or a backslash, so a user name with a space or a Windows path survives. Inside
    double quotes a backslash before another backslash or a quote is an escape, so every backslash
    is doubled: a UNC path (`\\\\server\\share`) comes out as it went in."""
    if not any(character in flag for character in ' \t"\'\\'):
        return flag
    if windows:
        return '"' + flag.replace("\\", "\\\\").replace('"', '\\"') + '"'
    return shlex.quote(flag)


def release_environment(environment=None, triple=None):
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
    windows = is_windows()
    existing = [quoted_flag(flag, windows) for flag in result.get("CFLAGS", "").split()]
    ours = [quoted_flag(flag, windows) for flag in c_flags(builder_paths(result))]
    result["CFLAGS"] = " ".join(existing + ours)
    result["CC_SHELL_ESCAPED_FLAGS"] = "1"
    if windows and triple:
        # Microsoft's compiler has no such flag, so every C file of a Windows release goes through
        # clang (LLVM is a build requirement already: `ring` needs it for Arm).
        result["CC_" + triple.replace("-", "_")] = "clang"
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
        environment = release_environment(base, triple)
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
