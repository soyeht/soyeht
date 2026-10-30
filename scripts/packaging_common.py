"""What every platform's package script shares (ADR 0121, ADR 0123).

The product version (`contracts/release.json`, ADR 0115), the profile a package ships, the names
of a package file, the release build of the two cargo workspaces and the programs it leaves. A
platform script (`package_linux.py`, `package_windows.py`) adds only what its format needs.
"""

import hashlib
import os
import platform
import subprocess
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
import release  # noqa: E402  (the scripts directory is on the path of a script)
from generated_profile import BUILD_ENVIRONMENT  # noqa: E402

# The production profile: the names a stranger's machine gets (ADR 0112).
PROFILE = "prod"
PACKAGE = "soyeht"
HOMEPAGE = "https://github.com/soyeht/soyeht"
# The programs of `engine/` that a package ships, by cargo package.
ENGINE_PACKAGES = ("soyeht-engine", "soyeht-ptyd", "soyeht-cli", "soyeht-mcp", "soyeht-service")


class PackagingError(Exception):
    pass


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
        return Path(out) / f"{PACKAGE}-{self.version}-{system}-{self.arch}.{extension}"


def make_plan(plan_class, arch, programs):
    """The plan of this commit's release: the version and revision come from the contract only."""
    made = release.current(ROOT)
    return plan_class(str(made.version), made.revision, arch, source_epoch(), programs)


def sha256_of(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


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


def cargo_build(engine_target, desktop_target, triple=None):
    """Release programs of both workspaces for the production profile; `--locked`, default
    features. `triple` builds for another target of the same system (Windows builds arm64 and x64
    on either)."""
    environment = dict(os.environ, **{BUILD_ENVIRONMENT: PROFILE})
    target = ["--target", triple] if triple else []
    for folder, directory, extra in (
        ("engine", engine_target, [arg for name in ENGINE_PACKAGES for arg in ("-p", name)]),
        ("apps/desktop", desktop_target, []),
    ):
        env = dict(environment, CARGO_TARGET_DIR=str(directory)) if directory else environment
        command = ["cargo", "build", "--release", "--locked", *target, *extra]
        subprocess.run(command, cwd=ROOT / folder, env=env, check=True)


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
