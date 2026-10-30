import json
import plistlib
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import package_mac
from generated_profile import PROFILES
from legacy_guard import names_legacy
from profile_programs import binary_identifiers

ROOT = Path(__file__).resolve().parents[1]
FULL = {
    package_mac.IDENTITY_VARIABLE: "Developer ID Application: Example (ABCDE12345)",
    package_mac.TEAM_VARIABLE: "ABCDE12345",
    package_mac.NOTARY_VARIABLE: "example-profile",
}
MACHO = b"\xcf\xfa\xed\xfe" + b"\0" * 28


def fake_app(root):
    """A bundle with the shape of the built app: one main executable, four programs, one
    framework with its own program, a resource that is not code and a link."""
    app = root / "Soyeht Next.app"
    macos = app / "Contents/MacOS"
    macos.mkdir(parents=True)
    (app / "Contents/Info.plist").write_bytes(
        plistlib.dumps({"CFBundleExecutable": "Soyeht Next", "CFBundleIdentifier": package_mac.APP_ID})
    )
    for name in ("Soyeht Next", "soyeht-engine", "soyeht-ptyd", "soyeht", "soyeht-mcp"):
        (macos / name).write_bytes(MACHO)
    framework = app / "Contents/Frameworks/Inner.framework/Versions/A"
    framework.mkdir(parents=True)
    (framework / "Inner").write_bytes(MACHO)
    (app / "Contents/Resources").mkdir()
    (app / "Contents/Resources/default.metallib").write_bytes(b"MTLB")
    (app / "Contents/Resources/link").symlink_to(macos / "soyeht")
    return app


class SigningChoiceTests(unittest.TestCase):
    def test_the_environment_alone_makes_a_developer_id_signing(self):
        signing = package_mac.resolve_signing(FULL)
        self.assertFalse(signing.ad_hoc)
        self.assertTrue(signing.notarizes)
        self.assertEqual(signing.team, "ABCDE12345")

    def test_no_identity_means_an_ad_hoc_build_that_says_so_loudly(self):
        for environment in ({}, {package_mac.TEAM_VARIABLE: "ABCDE12345"}):
            signing = package_mac.resolve_signing(environment)
            self.assertTrue(signing.ad_hoc)
            self.assertFalse(signing.notarizes)
            text = "\n".join(package_mac.banner(signing))
            self.assertIn("AD-HOC", text)
            self.assertIn("NOTARIZATION IS SKIPPED", text)

    def test_ad_hoc_ignores_the_environment_and_release_refuses_to_fall_back(self):
        self.assertTrue(package_mac.resolve_signing(FULL, ad_hoc=True).ad_hoc)
        with self.assertRaisesRegex(SystemExit, "needs .*SOYEHT_MAC_SIGNING_IDENTITY"):
            package_mac.resolve_signing({}, strict=True)
        with self.assertRaisesRegex(SystemExit, "contradict"):
            package_mac.resolve_signing(FULL, ad_hoc=True, strict=True)

    def test_a_release_names_every_missing_variable_and_never_a_value(self):
        with self.assertRaises(SystemExit) as caught:
            package_mac.resolve_signing({package_mac.IDENTITY_VARIABLE: "x"}, strict=True)
        self.assertIn(package_mac.TEAM_VARIABLE, str(caught.exception))
        self.assertIn(package_mac.NOTARY_VARIABLE, str(caught.exception))
        self.assertNotIn("x", str(caught.exception).replace("SOYEHT_MAC_SIGNING_IDENTITY", ""))

    def test_an_identity_needs_its_team_and_must_be_a_developer_id(self):
        identity = {package_mac.IDENTITY_VARIABLE: FULL[package_mac.IDENTITY_VARIABLE]}
        with self.assertRaisesRegex(SystemExit, "TEAM"):
            package_mac.resolve_signing(identity)
        with self.assertRaisesRegex(SystemExit, "ten-character"):
            package_mac.resolve_signing({**FULL, package_mac.TEAM_VARIABLE: "short"})
        with self.assertRaisesRegex(SystemExit, "Developer ID Application"):
            package_mac.resolve_signing(
                {**FULL, package_mac.IDENTITY_VARIABLE: "Apple Development: Example (ABCDE12345)"}
            )
        hashed = {**FULL, package_mac.IDENTITY_VARIABLE: "A" * 40}
        self.assertEqual(package_mac.resolve_signing(hashed).identity, "A" * 40)

    def test_a_signed_build_without_a_notary_profile_warns_that_it_is_not_notarized(self):
        environment = {key: value for key, value in FULL.items() if key != package_mac.NOTARY_VARIABLE}
        text = "\n".join(package_mac.banner(package_mac.resolve_signing(environment)))
        self.assertIn("NOT NOTARIZED", text)


class NamesAndOutputTests(unittest.TestCase):
    def test_the_image_is_named_from_the_version_the_way_every_platform_is(self):
        self.assertEqual(package_mac.artifact_name("1.2.3", "dmg"), "soyeht-1.2.3-macos-aarch64.dmg")

    def test_an_ad_hoc_build_can_never_land_beside_the_release(self):
        dist = Path("dist")
        self.assertEqual(package_mac.output_directory(package_mac.Signing(), dist), dist / "ad-hoc")
        self.assertEqual(package_mac.output_directory(package_mac.resolve_signing(FULL), dist), dist)

    def test_every_identifier_of_the_package_carries_next_and_none_is_legacy(self):
        identifiers = binary_identifiers(PROFILES["prod"])
        self.assertEqual(identifiers["engine"], "com.soyeht.next.engine")
        self.assertEqual(identifiers["ptyd"], "com.soyeht.next.ptyd")
        self.assertEqual(set(identifiers), {key for _, key in package_mac.PROGRAMS})
        for value in (*identifiers.values(), package_mac.APP_ID, package_mac.PRODUCT_NAME):
            self.assertIn("next", value.lower())
            self.assertFalse(names_legacy(value))
        self.assertFalse(names_legacy(f"/Applications/{package_mac.PRODUCT_NAME}.app"))
        self.assertEqual(package_mac.APP_ID, "com.soyeht.next.mac")

    def test_the_app_is_built_with_the_production_identity_and_the_contracts_version(self):
        made = package_mac.release.current()
        settings = package_mac.xcode_settings(made)
        self.assertEqual(settings["SOYEHT_APP_BUNDLE_ID"], PROFILES["prod"]["appIds"]["macos"])
        self.assertEqual(settings["MARKETING_VERSION"], str(made.version))
        self.assertEqual(settings["CURRENT_PROJECT_VERSION"], str(made.build_number()))
        self.assertEqual(settings["SOYEHT_PRODUCT_NAME"], package_mac.PRODUCT_NAME)
        self.assertNotIn("PRODUCT_NAME", settings, "a global product name renames every package target")
        self.assertEqual(settings["ARCHS"], "arm64")
        self.assertEqual(settings["CODE_SIGNING_ALLOWED"], "NO", "the script signs, not Xcode")


class SigningCallTests(unittest.TestCase):
    def test_every_call_asks_for_the_hardened_runtime(self):
        for signing in (package_mac.Signing(), package_mac.resolve_signing(FULL)):
            command = package_mac.codesign_command(Path("x"), signing)
            self.assertEqual(command[command.index("--options") + 1], "runtime")

    def test_a_developer_id_call_has_a_secure_timestamp_and_an_ad_hoc_call_none(self):
        signed = package_mac.codesign_command(Path("x"), package_mac.resolve_signing(FULL))
        self.assertIn("--timestamp", signed)
        self.assertEqual(signed[signed.index("--sign") + 1], FULL[package_mac.IDENTITY_VARIABLE])
        adhoc = package_mac.codesign_command(Path("x"), package_mac.Signing())
        self.assertIn("--timestamp=none", adhoc)
        self.assertEqual(adhoc[adhoc.index("--sign") + 1], "-")

    def test_identifier_and_entitlements_are_passed_only_when_given(self):
        command = package_mac.codesign_command(
            Path("x"), package_mac.Signing(), identifier="id", entitlements=Path("e.plist")
        )
        self.assertEqual(command[command.index("--identifier") + 1], "id")
        self.assertEqual(command[command.index("--entitlements") + 1], Path("e.plist"))
        self.assertNotIn("--entitlements", package_mac.codesign_command(Path("x"), package_mac.Signing()))

    def test_nested_code_is_found_deepest_first_without_the_main_executable_or_links(self):
        with tempfile.TemporaryDirectory() as temporary:
            app = fake_app(Path(temporary))
            found = package_mac.nested_code(app)
            names = [path.name for path in found]
            self.assertNotIn("Soyeht Next", names)
            self.assertNotIn("link", names)
            self.assertNotIn("default.metallib", names)
            self.assertLess(names.index("Inner"), names.index("Inner.framework"))
            self.assertEqual(
                set(names), {"Inner", "Inner.framework", "soyeht", "soyeht-engine", "soyeht-mcp", "soyeht-ptyd"}
            )

    def test_the_programs_are_signed_under_their_identifiers_then_the_bundle_with_its_entitlements(self):
        with tempfile.TemporaryDirectory() as temporary:
            app = fake_app(Path(temporary))
            calls = []
            with patch.object(package_mac, "run", side_effect=lambda command, **_: calls.append(command)):
                package_mac.sign_bundle(app, package_mac.resolve_signing(FULL))
        signed = [Path(command[-1]) for command in calls]
        self.assertEqual(signed[-1], app, "the bundle is signed last")
        identifier = {Path(c[-1]).name: c[c.index("--identifier") + 1] for c in calls if "--identifier" in c}
        self.assertEqual(identifier["soyeht-engine"], "com.soyeht.next.engine")
        self.assertEqual(identifier["soyeht-ptyd"], "com.soyeht.next.ptyd")
        self.assertEqual(identifier["soyeht"], "com.soyeht.next.cli")
        self.assertEqual(identifier["soyeht-mcp"], "com.soyeht.next.mcp")
        self.assertNotIn("--deep", [part for command in calls for part in command])
        entitled = [Path(c[-1]).name for c in calls if "--entitlements" in c]
        self.assertEqual(entitled, ["Soyeht Next.app"], "no program carries an entitlement")

    def test_a_program_that_is_missing_stops_the_assembly(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            app = fake_app(root)
            with patch.object(package_mac, "run"):
                with self.assertRaisesRegex(SystemExit, "build the engine first"):
                    package_mac.assemble(app, {"soyeht-engine": root / "absent"}, root / "out/Soyeht Next.app")


class CheckTests(unittest.TestCase):
    DETAILS = (
        "Executable=/x\nIdentifier=com.soyeht.next.engine\nFormat=Mach-O thin (arm64)\n"
        "CodeDirectory v=20500 size=1 flags=0x10000(runtime) hashes=2+0 location=embedded\n"
        "TeamIdentifier=ABCDE12345\n"
    )

    def test_a_signature_is_checked_for_identifier_team_and_runtime(self):
        self.assertEqual(
            package_mac.check_signature(self.DETAILS, identifier="com.soyeht.next.engine", team="ABCDE12345"), []
        )
        wrong = package_mac.check_signature(self.DETAILS, identifier="other", team="ZZZZZ99999")
        self.assertEqual(len(wrong), 2)
        plain = self.DETAILS.replace("0x10000(runtime)", "0x0(none)")
        self.assertEqual(package_mac.check_signature(plain), ["hardened runtime is off"])

    def test_an_ad_hoc_signature_has_no_team_to_check(self):
        details = self.DETAILS.replace("0x10000(runtime)", "0x10002(adhoc,runtime)").replace(
            "ABCDE12345", "not set"
        )
        self.assertEqual(package_mac.check_signature(details, identifier="com.soyeht.next.engine"), [])

    def test_the_bundle_info_must_match_the_contract_and_the_production_identity(self):
        made = package_mac.release.current()
        with tempfile.TemporaryDirectory() as temporary:
            app = Path(temporary) / "A.app"
            (app / "Contents").mkdir(parents=True)
            good = {
                "CFBundleIdentifier": package_mac.APP_ID,
                "CFBundleShortVersionString": str(made.version),
                "CFBundleVersion": str(made.build_number()),
            }
            (app / "Contents/Info.plist").write_bytes(plistlib.dumps(good))
            self.assertEqual(package_mac.check_info(app, made), [])
            bad = {**good, "CFBundleIdentifier": "com.soyeht.next.dev.mac", "CFBundleVersion": "1"}
            (app / "Contents/Info.plist").write_bytes(plistlib.dumps(bad))
            self.assertEqual(len(package_mac.check_info(app, made)), 2)

    def test_a_notary_verdict_that_is_not_json_is_an_empty_answer(self):
        self.assertEqual(package_mac.parse_verdict("not json"), {})
        self.assertEqual(package_mac.parse_verdict("[1]"), {})
        self.assertEqual(package_mac.parse_verdict(json.dumps({"status": "Accepted"})), {"status": "Accepted"})

    def test_a_rejected_notarization_stops_the_run_and_prints_the_log(self):
        signing = package_mac.resolve_signing(FULL)
        answers = [
            type("R", (), {"stdout": json.dumps({"id": "abc", "status": "Invalid"}), "stderr": "", "returncode": 0})(),
            type("R", (), {"stdout": "the log", "stderr": "", "returncode": 0})(),
        ]
        with patch.object(package_mac, "run", side_effect=answers) as run:
            with self.assertRaisesRegex(SystemExit, "was not accepted: Invalid"):
                package_mac.notarize(Path("a.zip"), signing)
        self.assertIn("abc", run.call_args_list[1].args[0])


class EntitlementTests(unittest.TestCase):
    def test_the_app_asks_for_exactly_the_entitlements_the_adr_justifies(self):
        entitlements = plistlib.loads(package_mac.ENTITLEMENTS.read_bytes())
        self.assertEqual(set(entitlements), package_mac.EXPECTED_APP_ENTITLEMENTS)
        self.assertTrue(all(value is True for value in entitlements.values()))
        adr = (ROOT / "docs/adr/0120-mac-release-packaging-and-the-bundled-services.md").read_text()
        for name in entitlements:
            self.assertIn(name, adr)

    def test_the_microphone_text_the_entitlement_serves_is_in_the_app(self):
        info = plistlib.loads((ROOT / "apps/mac/Resources/Info.plist").read_bytes())
        self.assertIn("NSMicrophoneUsageDescription", info)


if __name__ == "__main__":
    unittest.main()
