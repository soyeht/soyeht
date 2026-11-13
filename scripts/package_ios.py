#!/usr/bin/env python3
"""Archive, sign and export the iPhone and iPad app of the production profile as an App Store Connect `.ipa`.

    uv run python scripts/package_ios.py             # archive, sign, export, check
    uv run python scripts/package_ios.py --validate  # and ask App Store Connect to validate it
    uv run python scripts/package_ios.py --verify-only dist/ios/soyeht-<version>-ios-aarch64.ipa

The result is `dist/ios/soyeht-<version>-ios-aarch64.ipa` (and its `.sha256`), the version being
`productVersion` of `contracts/release.json` and the build number the one of its revision. It
goes into `dist/ios/`, not `dist/`, because it is not a package of the GitHub release
(`release_flow.py` collects the files of `dist/`): the app is delivered through the App Store. The
script stops before the upload; nothing here uploads, and `--validate` only asks App Store Connect
whether it would accept the file. Everything secret comes from the environment:

    SOYEHT_IOS_SIGNING_IDENTITY  the name or SHA-1 of an "Apple Distribution" certificate
    SOYEHT_DEVELOPMENT_TEAM      its ten-character team identifier
    SOYEHT_IOS_PROFILE           optional: the name of the App Store provisioning profile, when it is
                                 not the one `apps/ios/ExportOptions.plist` names

and for `--validate`, an App Store Connect API key (the one that notarizes the Mac app):

    SOYEHT_NOTARY_KEY_FILE       the path of the `.p8` private key (kept outside the repository)
    SOYEHT_NOTARY_KEY_ID         its key identifier
    SOYEHT_NOTARY_ISSUER         the issuer identifier of the team's keys

The steps (docs/apps-iphone.md, "Store build"): `xcodebuild archive` of the Release configuration
with no signing (a manual profile reaches the Swift package targets, which take none); the archived
app signed with the distribution identity and the entitlements of `apps/ios/SoyehtPhone.entitlements`
completed with the identity keys an unsigned archive lacks; `xcodebuild -exportArchive` for the
`app-store-connect` method; and a check of the file: its identifier, version and build number, its
entitlements, that it is signed with a distribution certificate and that its profile is an App Store
one. The certificate and the profile are made in the developer account and never kept here.
"""

from __future__ import annotations

import argparse
import os
import plistlib
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

import release
from package_common import (
    HASH_PATTERN,
    PRODUCT_NAME,
    ROOT,
    TEAM_PATTERN,
    TEAM_VARIABLE,
    PackagingError,
    api_key,
    app_build_settings,
    artifact_path,
    check_sizes,
    checked_release,
    entitlement_values,
    info_problems,
    run,
    signature_details,
    stream,
    verification_status,
    verify_team,
    write_checksum,
    xcodebuild_command,
)

APP_ID = "com.soyeht.next.ios"  # the production identifier of the iOS app (ADR 0112); not in `profiles.json`
OPERATING_SYSTEM = "ios"
ARTIFACT_ARCHITECTURE = "aarch64"  # what the package name calls the one architecture of an iPhone
XCODE_PROJECT = ROOT / "apps/ios/SoyehtPhone.xcodeproj"
SCHEME = "SoyehtPhone"
ENTITLEMENTS = ROOT / "apps/ios/SoyehtPhone.entitlements"
EXPORT_OPTIONS = ROOT / "apps/ios/ExportOptions.plist"
WORK = ROOT / ".cache/package-ios"
DIST = ROOT / "dist" / "ios"

IDENTITY_VARIABLE = "SOYEHT_IOS_SIGNING_IDENTITY"
PROFILE_VARIABLE = "SOYEHT_IOS_PROFILE"
CERTIFICATE_KIND = "Apple Distribution:"
# What an exported app may be signed with beyond the identity keys: the profile adds the keychain
# access group and TestFlight's reports flag. A key outside this set is a capability nobody asked for.
IDENTITY_KEYS = ("application-identifier", "com.apple.developer.team-identifier", "get-task-allow")
PROFILE_KEYS = ("keychain-access-groups", "beta-reports-active")
PUSH_KEY = "aps-environment"
PUSH_PRODUCTION = "production"
NESTED_SUFFIXES = {".framework", ".appex", ".xpc", ".dylib"}


@dataclass(frozen=True)
class Signing:
    """The identity, the team and the profile the export is made with."""

    identity: str
    team: str
    profile: str | None = None  # None: the one `ExportOptions.plist` names

    @property
    def secrets(self):
        """What a printed command must not show: the certificate's name holds a person's name."""
        return (self.identity,)


def resolve_signing(environment):
    """The signing the environment asks for; an iPhone build has no ad-hoc form, so a missing
    identity or team is an error."""
    identity = environment.get(IDENTITY_VARIABLE) or None
    team = environment.get(TEAM_VARIABLE) or None
    missing = [name for name, value in ((IDENTITY_VARIABLE, identity), (TEAM_VARIABLE, team)) if not value]
    if missing:
        raise PackagingError(f"an App Store build needs {', '.join(missing)} in the environment")
    if not TEAM_PATTERN.match(team):
        raise PackagingError(f"{TEAM_VARIABLE} must be a ten-character team identifier")
    if not (identity.startswith(CERTIFICATE_KIND) or HASH_PATTERN.match(identity)):
        raise PackagingError(
            f'{IDENTITY_VARIABLE} must name an "{CERTIFICATE_KIND}" certificate or its SHA-1 hash: '
            "only that kind can be exported for the App Store"
        )
    return Signing(identity, team, environment.get(PROFILE_VARIABLE) or None)


# --- building ---------------------------------------------------------------------------------


def archive_path():
    return WORK / f"{PRODUCT_NAME}.xcarchive"


def xcode_settings(made):
    """The shared production settings (`app_build_settings`): an iPhone has one architecture to name."""
    return app_build_settings(made, APP_ID, PRODUCT_NAME)


def archive_app(made, environment):
    """The Release archive, unsigned. The signing happens on the archived app (`sign_app`)."""
    if archive_path().exists():
        shutil.rmtree(archive_path())
    WORK.mkdir(parents=True, exist_ok=True)
    command = xcodebuild_command(
        XCODE_PROJECT, SCHEME, "generic/platform=iOS", WORK / "derived", xcode_settings(made),
        "-archivePath", archive_path(), "archive",
    )
    stream(command, environment=environment)
    app = archived_app(archive_path())
    if not app.is_dir():
        raise PackagingError(f"xcodebuild did not make {app}")
    return app


def archived_app(archive):
    return archive / "Products/Applications" / f"{PRODUCT_NAME}.app"


# --- signing --------------------------------------------------------------------------------------


def signing_entitlements(source, team):
    """What the archived app is signed with: the entitlements of the repository's file plus the
    identity keys an unsigned archive lacks. Push is production for a store build: the file says
    `development`, and Xcode switches it on export. A file with no push key gives an app with none."""
    entitlements = dict(source)
    entitlements.update(
        {"application-identifier": f"{team}.{APP_ID}", "com.apple.developer.team-identifier": team, "get-task-allow": False}
    )
    if PUSH_KEY in entitlements:
        entitlements[PUSH_KEY] = PUSH_PRODUCTION
    return entitlements


def nested_code(app):
    """Every framework, extension and library inside `app`, the deepest first, so a bundle is
    signed after the code it holds."""
    found = [path for path in app.rglob("*") if path.suffix in NESTED_SUFFIXES and not path.is_symlink()]
    return sorted(found, key=lambda path: len(path.parts), reverse=True)


def codesign_command(path, signing, entitlements=None):
    command = ["codesign", "--force", "--sign", signing.identity, "--timestamp"]
    if entitlements:
        command += ["--entitlements", entitlements]
    return [*command, path]


def sign_app(app, signing):
    """Sign what the app holds, then the app with its entitlements. The entitlements file holds the
    team, so it lives in a folder that is removed when the signing ends."""
    with tempfile.TemporaryDirectory(prefix="soyeht-ios-entitlements-") as private:
        entitlements = Path(private) / "signing.entitlements"
        entitlements.write_bytes(plistlib.dumps(signing_entitlements(plistlib.loads(ENTITLEMENTS.read_bytes()), signing.team)))
        for path in nested_code(app):
            run(codesign_command(path, signing), mask=signing.secrets)
        run(codesign_command(app, signing, entitlements), mask=signing.secrets)


# --- exporting ------------------------------------------------------------------------------------


def export_options(template, signing):
    """The options of this export: the committed file with the team and the identity of the
    environment (and the profile, when the environment names another) added. Neither is in the file."""
    options = plistlib.loads(template)
    profiles = options.get("provisioningProfiles") or {}
    if APP_ID not in profiles:
        raise PackagingError(f"{EXPORT_OPTIONS.name} names no profile for {APP_ID}")
    options["teamID"] = signing.team
    options["signingCertificate"] = signing.identity
    if signing.profile:
        profiles[APP_ID] = signing.profile
    return options


def export_archive(signing, scratch):
    """`xcodebuild -exportArchive` of the signed archive; returns the `.ipa` it wrote."""
    folder = scratch / "export"
    if folder.exists():
        shutil.rmtree(folder)
    with tempfile.TemporaryDirectory(prefix="soyeht-ios-export-") as private:  # the options hold the team and the identity
        options = Path(private) / "ExportOptions.plist"
        options.write_bytes(plistlib.dumps(export_options(EXPORT_OPTIONS.read_bytes(), signing)))
        stream(
            ["xcodebuild", "-exportArchive", "-archivePath", archive_path(), "-exportPath", folder, "-exportOptionsPlist", options],
            mask=signing.secrets,
        )
    found = sorted(folder.glob("*.ipa"))
    if len(found) != 1:
        raise PackagingError(f"the export wrote {len(found)} .ipa files in {folder}, not one")
    return found[0]


def save_symbols(archive, destination):
    """The debug symbols of the archive, zipped: a crash of the app names its frames only with
    them. It is not part of the package. Returns the file, or None when the archive has none."""
    symbols = archive / "dSYMs"
    if not symbols.is_dir():
        return None
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.unlink(missing_ok=True)
    run(["ditto", "-c", "-k", "--norsrc", "--keepParent", symbols, destination])
    return destination


# --- checking the result ---------------------------------------------------------------------------


def declared_entitlements():
    """The entitlement keys the repository's file declares: the ones the exported app must carry."""
    return set(plistlib.loads(ENTITLEMENTS.read_bytes()))


def check_entitlements(entitled, team, declared):
    """The entitlements of the exported app against what an App Store build of this app holds: the
    identity keys, every key the repository's file `declared` (push must be `production`) and, beyond
    those, only what the profile adds."""
    problems = []
    wanted = {
        "application-identifier": f"{team}.{APP_ID}",
        "com.apple.developer.team-identifier": team,
        "get-task-allow": False,
    }
    problems += [f"{key} is {entitled.get(key)!r}, not {value!r}" for key, value in wanted.items() if entitled.get(key) != value]
    problems += [f"{key} is missing: the app declares it" for key in sorted(declared - set(IDENTITY_KEYS) - set(entitled))]
    if PUSH_KEY in declared and PUSH_KEY in entitled and entitled[PUSH_KEY] != PUSH_PRODUCTION:
        problems.append(f"{PUSH_KEY} is {entitled[PUSH_KEY]!r}: an App Store build uses {PUSH_PRODUCTION!r}")
    allowed = {*IDENTITY_KEYS, *PROFILE_KEYS, *declared}
    problems += [f"unexpected entitlement {key}" for key in sorted(set(entitled) - allowed)]
    return problems


def check_signature(details, team):
    """What `codesign -dvv` says: a distribution certificate and the team's identifier."""
    problems = []
    if not any(line.startswith("Authority=Apple Distribution") for line in details.splitlines()):
        problems.append("it is not signed with an Apple Distribution certificate")
    if f"TeamIdentifier={team}\n" not in details:
        problems.append(f"team is not {team}")
    return problems


def check_profile(profile, team):
    """The embedded provisioning profile of an App Store build: for the app's identifier and team, for
    no listed devices (a development or ad-hoc profile has them) and with the distribution flag."""
    problems = []
    if profile.get("Entitlements", {}).get("application-identifier") != f"{team}.{APP_ID}":
        problems.append(f"the profile is not for {APP_ID}")
    if "ProvisionedDevices" in profile or profile.get("ProvisionsAllDevices"):
        problems.append("the profile is not an App Store profile (it lists devices)")
    if profile.get("Entitlements", {}).get("get-task-allow"):
        problems.append("the profile allows debugging (get-task-allow)")
    return problems


def embedded_profile(app):
    """The provisioning profile inside `app`, read through `security cms`."""
    path = app / "embedded.mobileprovision"
    if not path.is_file():
        raise PackagingError("the exported app carries no embedded.mobileprovision")
    return plistlib.loads(run(["security", "cms", "-D", "-i", path]).stdout.encode())


def verify_app(app, team, made):
    """Everything that can be checked on the exported app without installing it. Returns the problems."""
    run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", app])
    info = plistlib.loads((app / "Info.plist").read_bytes())
    problems = info_problems(info, APP_ID, made)
    problems += check_signature(signature_details(app), team)
    problems += check_entitlements(entitlement_values(app), team, declared_entitlements())
    problems += check_profile(embedded_profile(app), team)
    return problems


def verify_ipa(ipa, team, made):
    """Unpack the `.ipa` into a scratch folder and check the one app in it."""
    with tempfile.TemporaryDirectory(prefix="soyeht-ipa-") as scratch:
        run(["ditto", "-x", "-k", ipa, scratch])
        apps = sorted((Path(scratch) / "Payload").glob("*.app"))
        if len(apps) != 1:
            return [f"{ipa.name} holds {len(apps)} apps in Payload/, not one"]
        problems = [] if apps[0].name == f"{PRODUCT_NAME}.app" else [f"the app is called {apps[0].name}, not {PRODUCT_NAME}.app"]
        return problems + verify_app(apps[0], team, made)


# --- validating ----------------------------------------------------------------------------------


def validate_command(ipa, key):
    """`altool --validate-app`: App Store Connect checks the file and keeps nothing; never an upload."""
    key_file, key_id, issuer = key
    return ["xcrun", "altool", "--validate-app", ipa,
            "--api-key", key_id, "--api-issuer", issuer, "--p8-file-path", key_file]


def validate(ipa, key):
    """The key's file, identifier and issuer are not printed with the command."""
    run(validate_command(ipa, key), timeout=1800, mask=key)


# --- the whole run ------------------------------------------------------------------------------


def parser():
    argument = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    argument.add_argument("--validate", action="store_true", help="ask App Store Connect to validate the .ipa (needs the API key variables; never uploads)")
    argument.add_argument("--verify-only", type=Path, metavar="IPA", help="check an exported .ipa of the current release version, with the team from the environment; builds nothing")
    argument.add_argument("--dist", type=Path, default=DIST, help="the output folder (default: dist/ios/)")
    return argument


def verify_only(ipa, environment):
    """Check an exported `.ipa` without building anything. Only the team comes from the
    environment; the identity that signed it is not needed."""
    team = verify_team(environment)
    if not ipa.is_file():
        raise PackagingError(f"--verify-only: {ipa} is not a file")
    return verification_status(ipa, verify_ipa(ipa, team, release.current()), "checks hold")


def package(options, environment):
    if options.verify_only:
        return verify_only(options.verify_only, environment)
    made = checked_release(APP_ID)
    signing = resolve_signing(environment)
    key = api_key(environment) if options.validate else None
    if options.validate and not key:
        raise PackagingError("--validate needs the App Store Connect API key variables in the environment (see --help)")
    app = archive_app(made, environment)
    sign_app(app, signing)
    exported = export_archive(signing, WORK)
    folder = options.dist
    ipa = artifact_path(folder, made.version, OPERATING_SYSTEM, ARTIFACT_ARCHITECTURE, "ipa")
    ipa.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(exported, ipa)
    symbols = save_symbols(archive_path(), artifact_path(folder / "symbols", made.version, OPERATING_SYSTEM, ARTIFACT_ARCHITECTURE, "dSYM.zip"))
    problems = verify_ipa(ipa, signing.team, made)
    checksum = write_checksum(ipa)
    print(f"\nIPA       {ipa}\nChecksum  {checksum}")
    print(f"Symbols   {symbols or 'the archive has none'}  (keep it with the release; it is not part of the package)")
    print("Signing   Apple Distribution")
    for problem in problems:
        print(f"PROBLEM: {problem}", file=sys.stderr)
    check_sizes([ipa])
    if key and not problems:
        validate(ipa, key)
        print("Validation  App Store Connect accepted the file. It was not uploaded.")
    return 1 if problems else 0


def main(argv=None):
    try:
        return package(parser().parse_args(argv), os.environ)
    except PackagingError as failure:
        print(f"package_ios: {failure}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
