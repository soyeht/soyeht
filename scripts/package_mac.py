#!/usr/bin/env python3
"""Build, sign, notarize and wrap the Mac app of the production profile (ADR 0120).

    uv run python scripts/package_mac.py            # signs with the identity of the environment
    uv run python scripts/package_mac.py --ad-hoc   # no identity: a build to test the pipeline
    uv run python scripts/package_mac.py --release  # refuses to go on without the identity

The result is `dist/soyeht-<version>-macos-aarch64.dmg` (and its `.sha256`), the version being
`productVersion` of `contracts/release.json`. An ad-hoc build goes to `dist/ad-hoc/`, so it can
never be taken for the release. The identity, the team and the notary credentials come only from
the environment:

    SOYEHT_MAC_SIGNING_IDENTITY  the name or SHA-1 of a "Developer ID Application" certificate
    SOYEHT_DEVELOPMENT_TEAM      its ten-character team identifier
    SOYEHT_NOTARY_PROFILE        a keychain profile made with `xcrun notarytool store-credentials`

The steps: the engine programs in release for Apple silicon, the app in Release for the
production profile, the programs inside the bundle (`Contents/MacOS`, beside the app's own
executable), every nested program signed and then the app (hardened runtime, secure timestamp),
the app notarized and stapled, a disk image of it signed, notarized and stapled, and a check of
the result. Nothing here opens the app, installs it or registers a service.
"""

from __future__ import annotations

import argparse
import json
import os
import plistlib
import re
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

import release
from generated_profile import PROFILES
from legacy_guard import names_legacy, require_next
from package_common import (
    PROFILE_NAME,
    ROOT,
    PackagingError,
    artifact_path,
    release_environment,
    run,
    stream,
    write_checksum,
)
from profile_programs import binary_identifiers

PROFILE = PROFILES[PROFILE_NAME]
APP_ID = PROFILE["appIds"]["macos"]
PRODUCT_NAME = PROFILE["displayNames"]["macApp"]
VOLUME_NAME = PRODUCT_NAME
OPERATING_SYSTEM = "macos"
ARCHITECTURE = "arm64"  # what `lipo` and Xcode call it
ARTIFACT_ARCHITECTURE = "aarch64"  # what the package name and the update manifest call it
RUST_TARGET = "aarch64-apple-darwin"
ENTITLEMENTS = ROOT / "apps/mac/Soyeht.entitlements"
XCODE_PROJECT = ROOT / "apps/mac/SoyehtMac.xcodeproj"
WORK = ROOT / ".cache/package-mac"
PACKAGES = ROOT / ".cache/apple-packages"
DIST = ROOT / "dist"
HELPERS_FOLDER = "Contents/MacOS"

IDENTITY_VARIABLE = "SOYEHT_MAC_SIGNING_IDENTITY"
TEAM_VARIABLE = "SOYEHT_DEVELOPMENT_TEAM"
NOTARY_VARIABLE = "SOYEHT_NOTARY_PROFILE"

# The programs inside the bundle: the cargo binary, the key of `binary_identifiers`.
PROGRAMS = (
    ("soyeht-engine", "engine"),
    ("soyeht-ptyd", "ptyd"),
    ("soyeht", "cli"),
    ("soyeht-mcp", "mcp"),
)
# What the app asks of hardened runtime, and why (ADR 0120). Nothing else is allowed.
EXPECTED_APP_ENTITLEMENTS = {"com.apple.security.device.audio-input"}
TEAM_PATTERN = re.compile(r"^[A-Z0-9]{10}$")
CERTIFICATE_KIND = "Developer ID Application:"
HASH_PATTERN = re.compile(r"^[0-9A-Fa-f]{40}$")
MACHO_MAGICS = (b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe")
NOTARY_TIMEOUT = "1h"


@dataclass(frozen=True)
class Signing:
    """How the build is signed. `identity` is None for an ad-hoc build."""

    identity: str | None = None
    team: str | None = None
    notary_profile: str | None = None

    @property
    def ad_hoc(self):
        return self.identity is None

    @property
    def notarizes(self):
        return self.notary_profile is not None


def resolve_signing(environment, ad_hoc=False, strict=False):
    """The signing the options and the environment ask for.

    An ad-hoc build ignores the environment. Without `--ad-hoc` an absent identity also means an
    ad-hoc build (the banner says so); `--release` turns that into an error. An identity without
    its team is an error either way, since the engine attests the app by team.
    """
    if ad_hoc and strict:
        raise PackagingError("--ad-hoc and --release contradict each other")
    if ad_hoc:
        return Signing()
    identity = environment.get(IDENTITY_VARIABLE) or None
    team = environment.get(TEAM_VARIABLE) or None
    notary = environment.get(NOTARY_VARIABLE) or None
    if strict:
        missing = [
            name
            for name, value in ((IDENTITY_VARIABLE, identity), (TEAM_VARIABLE, team), (NOTARY_VARIABLE, notary))
            if not value
        ]
        if missing:
            raise PackagingError(f"--release needs {', '.join(missing)} in the environment")
    if identity is None:
        return Signing()
    if team is None:
        raise PackagingError(f"{IDENTITY_VARIABLE} is set, so {TEAM_VARIABLE} must be set too")
    if not TEAM_PATTERN.match(team):
        raise PackagingError(f"{TEAM_VARIABLE} must be a ten-character team identifier")
    if not (identity.startswith(CERTIFICATE_KIND) or HASH_PATTERN.match(identity)):
        raise PackagingError(
            f'{IDENTITY_VARIABLE} must name a "{CERTIFICATE_KIND}" certificate or its SHA-1 hash: '
            "only that kind can be notarized"
        )
    return Signing(identity, team, notary)


def banner(signing):
    """The lines that say what this run will and will not produce."""
    if not signing.ad_hoc:
        lines = [f"Signing with the Developer ID identity of {IDENTITY_VARIABLE}, team {signing.team}."]
        if not signing.notarizes:
            lines += [
                f"{NOTARY_VARIABLE} is not set: the build is NOT NOTARIZED and Gatekeeper will "
                "warn on every other Mac.",
            ]
        return lines
    return [
        "*" * 78,
        "AD-HOC SIGNING: this build is for testing the pipeline ONLY.",
        "It has no Developer ID, so Gatekeeper rejects it on every other Mac,",
        "and the engine does not treat the app as the signed UI (it attests the team).",
        "NOTARIZATION IS SKIPPED. Do not give this file to anyone.",
        f"For a release set {IDENTITY_VARIABLE}, {TEAM_VARIABLE} and {NOTARY_VARIABLE}.",
        "*" * 78,
    ]


def output_directory(signing, dist=DIST):
    return dist if not signing.ad_hoc else dist / "ad-hoc"


# --- building ---------------------------------------------------------------------------------


def cargo_target_directory(environment):
    return Path(environment.get("CARGO_TARGET_DIR") or ROOT / "engine/target")


def program_path(name, environment):
    return cargo_target_directory(environment) / RUST_TARGET / "release" / name


def build_engine(environment):
    """The four programs, optimised, for Apple silicon, with the production profile built in
    (`SOYEHT_BUILD_PROFILE`, ADR 0112) so none of them can fall back to Dev data."""
    command = ["cargo", "build", "--release", "--locked", "--target", RUST_TARGET]
    for name, _ in PROGRAMS:
        command += ["--bin", name]
    stream(command, environment=release_environment(environment), cwd=ROOT / "engine")
    return {name: program_path(name, environment) for name, _ in PROGRAMS}


def xcode_settings(made):
    """The build settings that turn the project's Dev app into the production app. The project
    itself stays Dev (the engine's tests read its identifier); the version is the contract's.
    The names are `SOYEHT_*` settings of the app target: a global `PRODUCT_NAME` would rename
    the resource bundle of every package as well and make two commands produce one file."""
    return {
        "SOYEHT_APP_BUNDLE_ID": APP_ID,
        "SOYEHT_PRODUCT_NAME": PRODUCT_NAME,
        "MARKETING_VERSION": str(made.version),
        "CURRENT_PROJECT_VERSION": str(made.build_number()),
        "ARCHS": ARCHITECTURE,
        "ONLY_ACTIVE_ARCH": "NO",
        "CODE_SIGNING_ALLOWED": "NO",
        "SOYEHT_PACKAGES": str(PACKAGES),
    }


def build_app(made, environment):
    derived = WORK / "derived"
    settings = [f"{key}={value}" for key, value in xcode_settings(made).items()]
    stream(
        [
            "xcodebuild", "-project", XCODE_PROJECT, "-scheme", "SoyehtMac",
            "-configuration", "Release", "-destination", "generic/platform=macOS",
            "-derivedDataPath", derived, "-clonedSourcePackagesDirPath", PACKAGES,
            "-skipPackagePluginValidation", "-onlyUsePackageVersionsFromResolvedFile", *settings, "build",
        ],
        environment=environment,
    )
    app = derived / "Build/Products/Release" / f"{PRODUCT_NAME}.app"
    if not app.is_dir():
        raise PackagingError(f"xcodebuild did not make {app}")
    return app


# --- assembling and signing ---------------------------------------------------------------------


def is_macho(path):
    if path.is_symlink() or not path.is_file():
        return False
    with path.open("rb") as handle:
        return handle.read(4) in MACHO_MAGICS


def nested_code(app):
    """Every bundle and program inside `app` except its own executable, the deepest first, so
    a bundle is signed after the code it holds."""
    main = app / "Contents/MacOS" / plistlib.loads((app / "Contents/Info.plist").read_bytes())["CFBundleExecutable"]
    found = []
    for folder, directories, files in os.walk(app):
        base = Path(folder)
        found += [base / name for name in directories if (base / name).suffix in {".framework", ".xpc", ".appex", ".app"}]
        found += [base / name for name in files if is_macho(base / name) and base / name != main]
    return sorted(found, key=lambda path: len(path.parts), reverse=True)


def assemble(app, programs, destination):
    """A copy of the built app with the programs in `Contents/MacOS`, where the app finds them
    with `Bundle.url(forAuxiliaryExecutable:)`."""
    if destination.exists():
        shutil.rmtree(destination)
    destination.parent.mkdir(parents=True, exist_ok=True)
    run(["ditto", app, destination])
    for name, source in programs.items():
        if not source.is_file():
            raise PackagingError(f"{source} is missing: build the engine first")
        shutil.copy2(source, destination / HELPERS_FOLDER / name)
        (destination / HELPERS_FOLDER / name).chmod(0o755)
    return destination


def thin_libraries(app):
    """Keep only the arm64 slice of a library that Xcode copied in as a universal file (the Swift
    back-deployment libraries are), so the whole bundle is Apple silicon only and smaller. The
    signature goes first, and the signing step makes a new one."""
    for path in nested_code(app):
        if is_macho(path) and (archs := run(["lipo", "-archs", path]).stdout.split()) != [ARCHITECTURE]:
            if ARCHITECTURE not in archs:
                raise PackagingError(f"{path.name} has no {ARCHITECTURE} slice ({' '.join(archs)})")
            thin = path.with_name(path.name + ".thin")
            run(["lipo", "-thin", ARCHITECTURE, path, "-output", thin])
            thin.replace(path)


def codesign_command(path, signing, *, identifier=None, entitlements=None):
    """One `codesign` call: hardened runtime and, with an identity, a secure timestamp."""
    command = ["codesign", "--force", "--sign", signing.identity or "-", "--options", "runtime"]
    command.append("--timestamp=none" if signing.ad_hoc else "--timestamp")
    if identifier:
        command += ["--identifier", identifier]
    if entitlements:
        command += ["--entitlements", entitlements]
    return [*command, path]


def sign_bundle(app, signing):
    """Sign every nested program under its identifier, then the app with its entitlements."""
    by_name = helper_identifiers()
    for path in nested_code(app):
        identifier = by_name.get(path.name) if path.parent == app / HELPERS_FOLDER else None
        if identifier:
            require_next("signing identifier", identifier)
        run(codesign_command(path, signing, identifier=identifier))
    run(codesign_command(app, signing, entitlements=ENTITLEMENTS))


def sign_file(path, signing):
    """A disk image: a secure timestamp and no entitlements (ad-hoc images stay unsigned)."""
    if not signing.ad_hoc:
        run(["codesign", "--force", "--sign", signing.identity, "--timestamp", path])


# --- notarizing ---------------------------------------------------------------------------------


def notarize(path, signing):
    """Submit `path`, wait for the verdict, print the log when it is not `Accepted`."""
    submitted = run(
        ["xcrun", "notarytool", "submit", path, "--keychain-profile", signing.notary_profile,
         "--wait", "--timeout", NOTARY_TIMEOUT, "--output-format", "json"],
        check=False, timeout=3700,
    )
    verdict = parse_verdict(submitted.stdout)
    if verdict.get("status") != "Accepted":
        if verdict.get("id"):
            log = run(["xcrun", "notarytool", "log", verdict["id"], "--keychain-profile", signing.notary_profile], check=False)
            print(log.stdout or log.stderr)
        raise PackagingError(f"Notarization of {path.name} was not accepted: {verdict.get('status', 'no answer')}")


def parse_verdict(text):
    try:
        verdict = json.loads(text)
    except json.JSONDecodeError:
        return {}
    return verdict if isinstance(verdict, dict) else {}


def notarize_and_staple_app(app, signing, scratch):
    archive = scratch / f"{app.stem}.zip"
    run(["ditto", "-c", "-k", "--keepParent", app, archive])
    notarize(archive, signing)
    run(["xcrun", "stapler", "staple", app])
    run(["xcrun", "stapler", "validate", app])


def notarize_and_staple_image(image, signing):
    notarize(image, signing)
    run(["xcrun", "stapler", "staple", image])
    run(["xcrun", "stapler", "validate", image])


# --- the disk image -------------------------------------------------------------------------------


def make_image(app, destination, scratch):
    """A compressed image holding the app and a link to /Applications, made without Finder (so
    no window opens and no layout is scripted)."""
    folder = scratch / "image"
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)
    run(["ditto", app, folder / app.name])
    (folder / "Applications").symlink_to("/Applications")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.unlink(missing_ok=True)
    run(["hdiutil", "create", "-volname", VOLUME_NAME, "-srcfolder", folder, "-fs", "APFS",
         "-format", "UDZO", "-ov", destination])


# --- checking the result ----------------------------------------------------------------------


def entitlements_of(path):
    """The entitlement keys `path` is signed with."""
    result = run(["codesign", "-d", "--entitlements", ":-", path], check=False)
    text = result.stdout.strip()
    return set(plistlib.loads(text.encode()).keys()) if text.startswith("<?xml") else set()


def signature_details(path):
    return run(["codesign", "-dvv", path], check=False).stderr


def flags_of(details):
    """The `flags=` text of the code directory line, or None when there is no such line."""
    for line in details.splitlines():
        if "flags=" in line:
            return line.split("flags=", 1)[1]
    return None


def check_signature(details, *, identifier=None, team=None, runtime=True):
    """What `codesign -dvv` says, against what was asked; one line per disagreement. An answer
    with no code directory line (nothing signed, or an error) never passes the runtime check."""
    problems = []
    if identifier and f"Identifier={identifier}\n" not in details:
        problems.append(f"identifier is not {identifier}")
    if team and f"TeamIdentifier={team}\n" not in details:
        problems.append(f"team is not {team}")
    if runtime:
        flags = flags_of(details)
        if flags is None:
            problems.append("no signature was found")
        elif "runtime" not in flags:
            problems.append("hardened runtime is off")
    return problems


def check_architecture(path):
    archs = run(["lipo", "-archs", path]).stdout.split()
    return [] if archs == [ARCHITECTURE] else [f"{path.name} is {' '.join(archs)}, not {ARCHITECTURE}"]


def check_info(app, made):
    info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
    wanted = {
        "CFBundleIdentifier": APP_ID,
        "CFBundleShortVersionString": str(made.version),
        "CFBundleVersion": str(made.build_number()),
    }
    problems = [f"{key} is {info.get(key)!r}, not {value!r}" for key, value in wanted.items() if info.get(key) != value]
    if names_legacy(json.dumps(info, default=str)):
        problems.append("Info.plist names the legacy app")
    return problems


def helper_identifiers():
    """The signing identifier of each program inside the bundle, by file name."""
    identifiers = binary_identifiers(PROFILE)
    return {name: identifiers[key] for name, key in PROGRAMS}


def verify_bundle(app, signing, made):
    """Everything that can be checked on the app without opening it. Returns the problems.

    Every program, library and bundle inside the app is checked, not only the ones this script
    knows by name: each must carry the hardened runtime and the team, and none an entitlement.
    """
    problems = check_info(app, made)
    team = signing.team
    run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", app])
    problems += check_signature(signature_details(app), identifier=APP_ID, team=team)
    if (entitled := entitlements_of(app)) != EXPECTED_APP_ENTITLEMENTS:
        problems.append(f"app entitlements are {sorted(entitled)}")
    main = plistlib.loads((app / "Contents/Info.plist").read_bytes())["CFBundleExecutable"]
    problems += check_architecture(app / "Contents/MacOS" / main)
    identifiers = helper_identifiers()
    present = {path.name for path in (app / HELPERS_FOLDER).iterdir()}
    problems += [f"{name} is not in the bundle" for name in identifiers if name not in present]
    for path in nested_code(app):
        name = str(path.relative_to(app))
        own = identifiers.get(path.name) if path.parent == app / HELPERS_FOLDER else None
        problems += [f"{name}: {line}" for line in check_signature(signature_details(path), identifier=own, team=team)]
        if is_macho(path):
            problems += check_architecture(path)
        if entitled := entitlements_of(path):
            problems.append(f"{name} has entitlements {sorted(entitled)}")
    return problems


def assess(path, kind):
    """What Gatekeeper says; the text, whether it accepted or not."""
    command = ["spctl", "--assess", "-vv"]
    command += ["--type", "execute"] if kind == "app" else ["--type", "open", "--context", "context:primary-signature"]
    result = run([*command, path], check=False)
    return result.returncode == 0, (result.stderr or result.stdout).strip()


def verify_image(image, signing, made):
    """Mount the image read-only with no window, check what is inside, and detach it again."""
    with tempfile.TemporaryDirectory(prefix="soyeht-image-") as mount:
        run(["hdiutil", "attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint", mount, image])
        try:
            app = Path(mount) / f"{PRODUCT_NAME}.app"
            problems = [] if app.is_dir() else [f"{app.name} is not in the image"]
            if not (Path(mount) / "Applications").is_symlink():
                problems.append("the link to /Applications is missing")
            if app.is_dir():
                problems += verify_bundle(app, signing, made)
            return problems
        finally:
            run(["hdiutil", "detach", mount], check=False)


# --- the whole run ------------------------------------------------------------------------------


def parser():
    argument = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    argument.add_argument("--ad-hoc", action="store_true", help="sign ad hoc and skip notarization")
    argument.add_argument("--release", action="store_true", help="require the identity, the team and the notary profile")
    argument.add_argument("--reuse-build", action="store_true", help="reuse the programs and the app of an earlier run")
    argument.add_argument("--dist", type=Path, default=DIST, help="the output folder (default: dist/)")
    return argument


def package(options, environment):
    made = release.current()
    wrong = release.problems()
    if wrong:
        raise PackagingError("The version disagrees with contracts/release.json:\n" + "\n".join(wrong))
    require_next("bundle identifier", APP_ID)
    require_next("product name", PRODUCT_NAME)
    signing = resolve_signing(environment, options.ad_hoc, options.release)
    print("\n".join(banner(signing)), file=sys.stderr)
    programs = {name: program_path(name, environment) for name, _ in PROGRAMS}
    if not options.reuse_build:
        programs = build_engine(environment)
    built = (WORK / "derived/Build/Products/Release" / f"{PRODUCT_NAME}.app") if options.reuse_build else build_app(made, environment)
    WORK.mkdir(parents=True, exist_ok=True)
    app = assemble(built, programs, WORK / "stage" / f"{PRODUCT_NAME}.app")
    thin_libraries(app)
    sign_bundle(app, signing)
    if signing.notarizes:
        notarize_and_staple_app(app, signing, WORK)
    problems = verify_bundle(app, signing, made)
    folder = output_directory(signing, options.dist)
    image = artifact_path(folder, made.version, OPERATING_SYSTEM, ARTIFACT_ARCHITECTURE, "dmg")
    make_image(app, image, WORK)
    sign_file(image, signing)
    if signing.notarizes:
        notarize_and_staple_image(image, signing)
    problems += verify_image(image, signing, made)
    checksum = write_checksum(image)
    report(image, checksum, app, signing, problems)
    return 1 if problems else 0


def report(image, checksum, app, signing, problems):
    accepted, text = assess(app, "app")
    image_accepted, image_text = assess(image, "image")
    print(f"\nImage     {image}\nChecksum  {checksum}")
    print(f"Signing   {'ad hoc' if signing.ad_hoc else 'Developer ID, team ' + str(signing.team)}")
    print(f"Gatekeeper, app    {'accepted' if accepted else 'rejected'}: {text}")
    print(f"Gatekeeper, image  {'accepted' if image_accepted else 'rejected'}: {image_text}")
    if not signing.ad_hoc and signing.notarizes and not (accepted and image_accepted):
        problems.append("Gatekeeper rejects a build that was notarized")
    for problem in problems:
        print(f"PROBLEM: {problem}", file=sys.stderr)


def main(argv=None):
    try:
        return package(parser().parse_args(argv), os.environ)
    except PackagingError as failure:
        print(f"package_mac: {failure}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
