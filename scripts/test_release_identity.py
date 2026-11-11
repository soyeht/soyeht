import os
import plistlib
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

import package_mac
import release_identity as identity
from package_common import PackagingError
from test_package_mac import FULL, MACHO, fake_app, verify_recorded
from generated_profile import PROFILES

PREFIX = "com.soyeht.next"
TEAM = "ABCDE12345"
ENGINE = f"{PREFIX}.engine"

# What `codesign -dvv` and `codesign -d -r-` print for a Developer ID program (recorded shape,
# placeholder team).
SIGNED = (
    f"Executable=/x/soyeht-engine\nIdentifier={ENGINE}\nFormat=Mach-O thin (arm64)\n"
    "CodeDirectory v=20500 size=1 flags=0x10000(runtime) hashes=2+0 location=embedded\n"
    f"Authority=Developer ID Application: Example ({TEAM})\nTimestamp=5 Nov 2026 at 10:00:00\n"
    f"TeamIdentifier={TEAM}\n"
)
REQUIREMENT_TEXT = (
    "Executable=/x/soyeht-engine\n"
    f'designated => identifier "{ENGINE}" and anchor apple generic and '
    "certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and "
    f"certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ and certificate leaf[subject.OU] = {TEAM}\n"
)
# An Apple Development certificate (the Dev profile) pins the leaf's name.
DEVELOPMENT_REQUIREMENT = (
    f'identifier "{ENGINE}" and anchor apple generic and certificate leaf[subject.CN] = '
    '"Apple Development: Example (XYZ0123456)" and certificate 1[field.1.2.840.113635.100.6.2.1] /* exists */'
)
AD_HOC = (
    f"Executable=/x/soyeht-engine\nIdentifier={ENGINE}\nFormat=Mach-O thin (arm64)\n"
    "CodeDirectory v=20400 size=1 flags=0x10002(adhoc,runtime) hashes=2+0 location=embedded\n"
    "Signature=adhoc\nTeamIdentifier=not set\n"
)
AD_HOC_REQUIREMENT = '# designated => cdhash H"0123456789abcdef0123456789abcdef01234567"\n'


def problems(details=SIGNED, requirement_text=REQUIREMENT_TEXT, **options):
    options = {"identifier": ENGINE, "prefix": PREFIX, "team": TEAM, "executable": True, **options}
    return identity.code_problems(
        "Soyeht Next.app/Contents/MacOS/soyeht-engine",
        details=details, requirement=identity.requirement_of(requirement_text), **options,
    )


class RecordedOutputTests(unittest.TestCase):
    def test_a_developer_id_program_with_a_fixed_identifier_passes(self):
        self.assertEqual(problems(), [])

    def test_a_development_certificate_passes_on_the_signature_team(self):
        requirement = f"designated => {DEVELOPMENT_REQUIREMENT}\n"
        self.assertEqual(problems(requirement_text=requirement), [])

    def test_an_ad_hoc_program_fails_every_rule_it_breaks(self):
        text = "\n".join(problems(AD_HOC, AD_HOC_REQUIREMENT))
        self.assertIn("code hash (ad-hoc): every update is a new row", text)
        self.assertIn("the signature has no team (ad hoc)", text)

    def test_a_bare_program_signed_without_an_identifier_fails(self):
        # What cargo's linker leaves, and `codesign` keeps when `--identifier` is not given.
        derived = "soyeht_engine-55549c4d00ee1a2b"
        details = SIGNED.replace(f"Identifier={ENGINE}", f"Identifier={derived}")
        requirement = REQUIREMENT_TEXT.replace(ENGINE, derived)
        text = "\n".join(problems(details, requirement))
        self.assertIn(f"identifier is {derived}, not {ENGINE}", text)

    def test_an_unknown_program_beside_the_app_must_carry_the_profile_prefix(self):
        derived = "helper"
        details = SIGNED.replace(f"Identifier={ENGINE}", f"Identifier={derived}")
        requirement = REQUIREMENT_TEXT.replace(ENGINE, derived)
        self.assertEqual(
            problems(details, requirement, identifier=None),
            [f"identifier {derived} does not start with {PREFIX}."],
        )
        self.assertEqual(problems(details, requirement, identifier=None, executable=False), [])

    def test_a_requirement_that_does_not_name_the_identifier_fails(self):
        wrong = REQUIREMENT_TEXT.replace(f'identifier "{ENGINE}"', 'identifier "other"')
        self.assertEqual(
            problems(requirement_text=wrong), [f'the designated requirement does not name identifier "{ENGINE}"']
        )

    def test_a_requirement_with_only_an_apple_anchor_matches_other_developers(self):
        loose = f'designated => identifier "{ENGINE}" and anchor apple generic\n'
        self.assertEqual(
            problems(requirement_text=loose),
            ["the designated requirement pins no signer: any Apple-issued certificate would match"],
        )

    def test_a_requirement_with_no_apple_anchor_fails(self):
        self.assertIn(
            "the designated requirement has no Apple anchor",
            problems(requirement_text=f'designated => identifier "{ENGINE}"\n'),
        )

    def test_a_pinned_team_other_than_the_expected_one_fails(self):
        other = REQUIREMENT_TEXT.replace(TEAM, "ZZZZZ99999")
        text = "\n".join(problems(requirement_text=other))
        self.assertIn(f"pins team ZZZZZ99999, not {TEAM}", text)

    def test_the_team_on_the_signature_must_be_the_expected_one(self):
        details = SIGNED.replace(f"TeamIdentifier={TEAM}", "TeamIdentifier=ZZZZZ99999")
        self.assertEqual(problems(details), [f"team is ZZZZZ99999, not {TEAM}"])

    def test_no_signature_and_no_requirement_are_reported(self):
        self.assertEqual(identity.requirement_of("code object is not signed at all\n"), None)
        text = "\n".join(problems("code object is not signed at all\n", ""))
        self.assertIn("no signing identifier", text)
        self.assertIn("no designated requirement", text)


class PathTests(unittest.TestCase):
    def test_the_programs_sit_at_a_path_that_does_not_move_with_a_release(self):
        for path in (
            "Soyeht Next.app/Contents/MacOS/soyeht-engine",
            "Soyeht Next.app/Contents/Frameworks/Inner.framework/Versions/A/Inner",
            "Soyeht Next.app/Contents/Frameworks/libswiftCompatibilitySpan.dylib",
            "Soyeht Next.app/Contents/Frameworks/libc++.1.dylib",
        ):
            self.assertEqual(identity.path_problems(path, "0.3.1"), [], path)

    def test_a_version_in_the_bundle_or_a_program_name_fails(self):
        for path in (
            "Soyeht Next 0.3.1.app/Contents/MacOS/soyeht-engine",
            "Soyeht Next.app/Contents/MacOS/soyeht-engine-0.3",
            "Soyeht Next.app/Contents/MacOS/soyeht-v2.0.0-rc.1",
            "Soyeht Next.app/Contents/Frameworks/Inner.framework/Versions/1.2/Inner",
        ):
            self.assertIn("holds a version", "\n".join(identity.path_problems(path)), path)

    def test_the_release_version_is_a_version_even_without_dots(self):
        self.assertTrue(identity.path_problems("Soyeht Next 7.app/Contents/MacOS/x", "7"))

    def test_a_hash_in_a_path_fails(self):
        for path in (
            "Soyeht Next.app/Contents/MacOS/soyeht-engine-1a2b3c4d5e6f",
            "Soyeht Next.app/Contents/Helpers/55549c4d00ee/helper",
        ):
            self.assertIn("holds a hash", "\n".join(identity.path_problems(path)), path)

    def test_a_word_made_of_hex_letters_is_not_a_hash(self):
        self.assertEqual(identity.path_problems("Soyeht Next.app/Contents/Resources/defaced/x"), [])


def real_tools():
    return sys.platform == "darwin" and shutil.which("codesign") is not None and Path("/usr/bin/true").is_file()


def build_bundle(root):
    """A bundle with the shape of the release: the app's executable and the four programs, each a
    real Mach-O (a copy of a system program), so `codesign` has something to sign."""
    app = root / f"{package_mac.PRODUCT_NAME}.app"
    macos = app / "Contents/MacOS"
    macos.mkdir(parents=True)
    (app / "Contents/Info.plist").write_bytes(
        plistlib.dumps({"CFBundleExecutable": "app", "CFBundleIdentifier": package_mac.APP_ID})
    )
    for name in ("app", *(name for name, _ in package_mac.PROGRAMS)):
        shutil.copyfile("/usr/bin/true", macos / name)
        (macos / name).chmod(0o755)
    return app


@unittest.skipUnless(real_tools(), "needs codesign on macOS")
class RealSignatureTests(unittest.TestCase):
    """The same rules against what `codesign` really prints. The ad-hoc case needs no identity."""

    def check(self, app, signing):
        return package_mac.identity_problems(app, signing, package_mac.release.current())

    def test_an_ad_hoc_bundle_is_refused_for_every_program(self):
        with tempfile.TemporaryDirectory() as temporary:
            app = build_bundle(Path(temporary))
            package_mac.sign_bundle(app, package_mac.Signing())
            text = "\n".join(self.check(app, package_mac.Signing("(already signed)", TEAM)))
        for name, _ in package_mac.PROGRAMS:
            self.assertIn(f"Contents/MacOS/{name}: the designated requirement is a code hash", text)
        self.assertIn("the signature has no team (ad hoc)", text)

    @unittest.skipUnless(
        os.environ.get(package_mac.IDENTITY_VARIABLE) and os.environ.get(package_mac.TEAM_VARIABLE),
        "needs SOYEHT_MAC_SIGNING_IDENTITY and SOYEHT_DEVELOPMENT_TEAM (the signing may ask the keychain)",
    )
    def test_a_bundle_signed_with_the_environments_identity_passes(self):
        signing = package_mac.Signing(os.environ[package_mac.IDENTITY_VARIABLE], os.environ[package_mac.TEAM_VARIABLE])
        with tempfile.TemporaryDirectory() as temporary:
            app = build_bundle(Path(temporary))
            package_mac.sign_bundle(app, signing)
            self.assertEqual(self.check(app, signing), [])


class WiringTests(unittest.TestCase):
    """`verify_bundle` and `--verify-only` really apply the rules (recorded output, no codesign)."""

    def verify(self, app, signing, requirement_for=None, identifier_for=None):
        def details(path):
            name = Path(path).name
            return SIGNED.replace(ENGINE, (identifier_for or {}).get(name, ENGINE))

        def requirement(path):
            name = Path(path).name
            return (requirement_for or {}).get(name, REQUIREMENT_TEXT.replace(ENGINE, (identifier_for or {}).get(name, ENGINE)))

        return "\n".join(verify_recorded(app, signing, details, requirement))

    def test_a_code_hash_requirement_fails_a_signed_build_but_not_an_ad_hoc_one(self):
        with tempfile.TemporaryDirectory() as temporary:
            app = fake_app(Path(temporary))
            hashed = {"soyeht-ptyd": AD_HOC_REQUIREMENT}
            signed = self.verify(app, package_mac.resolve_signing(FULL), hashed)
            ad_hoc = self.verify(app, package_mac.Signing(), hashed)
        self.assertIn("soyeht-ptyd: the designated requirement is a code hash", signed)
        self.assertNotIn("code hash", ad_hoc)

    def test_a_bundle_with_another_name_than_the_product_fails(self):
        with tempfile.TemporaryDirectory() as temporary:
            app = fake_app(Path(temporary))
            renamed = app.rename(app.with_name("Other.app"))
            text = self.verify(renamed, package_mac.resolve_signing(FULL))
        self.assertIn(f"the bundle is called Other.app, not {package_mac.PRODUCT_NAME}.app", text)

    def test_a_program_beside_the_app_with_a_foreign_identifier_fails(self):
        with tempfile.TemporaryDirectory() as temporary:
            app = fake_app(Path(temporary))
            (app / "Contents/MacOS/stray").write_bytes(MACHO)
            text = self.verify(app, package_mac.resolve_signing(FULL), identifier_for={"stray": "org.example.stray"})
        self.assertIn(f"stray: identifier org.example.stray does not start with {PREFIX}.", text)

    def test_verify_only_needs_the_team_and_an_app(self):
        with tempfile.TemporaryDirectory() as temporary:
            app = fake_app(Path(temporary))
            with self.assertRaisesRegex(PackagingError, "needs SOYEHT_DEVELOPMENT_TEAM"):
                package_mac.package(package_mac.parser().parse_args(["--verify-only", str(app)]), {})
            environment = {package_mac.TEAM_VARIABLE: TEAM}
            with self.assertRaisesRegex(PackagingError, "is not an app bundle"):
                package_mac.package(
                    package_mac.parser().parse_args(["--verify-only", str(Path(temporary) / "absent.app")]), environment
                )


class ProfileTests(unittest.TestCase):
    def test_the_prefix_the_rules_use_is_the_profiles_and_carries_next(self):
        prod = PROFILES[package_mac.PROFILE_NAME]
        self.assertEqual(prod["launchdPrefix"], PREFIX)
        self.assertTrue(package_mac.APP_ID.startswith(f"{PREFIX}."))


if __name__ == "__main__":
    unittest.main()
