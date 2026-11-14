import contextlib
import io
import plistlib
import stat
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import package_common
import package_ios
from generated_legacy import BUNDLE_IDS
from legacy_guard import names_legacy
from package_common import PackagingError

TEAM = "ABCDE12345"
HASH = "0123456789abcdef0123456789abcdef01234567"
FULL = {
    package_ios.IDENTITY_VARIABLE: "Apple Distribution: Example (ABCDE12345)",
    package_common.TEAM_VARIABLE: TEAM,
}
GOOD_ENTITLEMENTS = {
    "application-identifier": f"{TEAM}.{package_ios.APP_ID}",
    "com.apple.developer.team-identifier": TEAM,
    "get-task-allow": False,
    "aps-environment": "production",
    "keychain-access-groups": [f"{TEAM}.*", "com.apple.token"],
    "beta-reports-active": True,
}
GOOD_DETAILS = f"Authority=Apple Distribution: Example ({TEAM})\nTeamIdentifier={TEAM}\n"
GOOD_PROFILE = {"Entitlements": {"application-identifier": f"{TEAM}.{package_ios.APP_ID}", "get-task-allow": False}}


def result(stdout="", stderr="", returncode=0):
    return type("Result", (), {"stdout": stdout, "stderr": stderr, "returncode": returncode})()


class SigningTests(unittest.TestCase):
    def test_the_environment_gives_the_identity_the_team_and_no_profile(self):
        signing = package_ios.resolve_signing(FULL)
        self.assertEqual((signing.identity, signing.team, signing.profile), (FULL[package_ios.IDENTITY_VARIABLE], TEAM, None))

    def test_a_certificate_can_be_named_by_its_hash_and_a_profile_can_be_chosen(self):
        signing = package_ios.resolve_signing({**FULL, package_ios.IDENTITY_VARIABLE: HASH, package_ios.PROFILE_VARIABLE: "Other"})
        self.assertEqual((signing.identity, signing.profile), (HASH, "Other"))

    def test_a_missing_identity_or_team_is_named(self):
        for name in (package_ios.IDENTITY_VARIABLE, package_common.TEAM_VARIABLE):
            environment = {key: value for key, value in FULL.items() if key != name}
            with self.assertRaisesRegex(PackagingError, name):
                package_ios.resolve_signing(environment)

    def test_there_is_no_ad_hoc_form(self):
        with self.assertRaises(PackagingError):
            package_ios.resolve_signing({})

    def test_a_certificate_of_another_kind_or_a_bad_team_is_refused(self):
        for identity in ("Apple Development: Example (ABCDE12345)", "Developer ID Application: Example (ABCDE12345)"):
            with self.assertRaisesRegex(PackagingError, "Apple Distribution"):
                package_ios.resolve_signing({**FULL, package_ios.IDENTITY_VARIABLE: identity})
        with self.assertRaisesRegex(PackagingError, "ten-character"):
            package_ios.resolve_signing({**FULL, package_common.TEAM_VARIABLE: "short"})


class BuildTests(unittest.TestCase):
    def test_the_app_is_the_production_one_with_the_contracts_version(self):
        made = package_ios.release.current()
        settings = package_ios.xcode_settings(made)
        self.assertEqual(settings["SOYEHT_APP_BUNDLE_ID"], "com.soyeht.next.ios")
        self.assertEqual(settings["SOYEHT_PRODUCT_NAME"], "Soyeht Next")
        self.assertEqual(settings["MARKETING_VERSION"], str(made.version))
        self.assertEqual(settings["CURRENT_PROJECT_VERSION"], str(made.build_number()))
        self.assertEqual(settings["CODE_SIGNING_ALLOWED"], "NO")

    def test_the_identifier_is_one_of_the_rewrite_and_not_the_legacy_apps(self):
        self.assertIn("next", package_ios.APP_ID)
        self.assertFalse(names_legacy(package_ios.APP_ID))

    def test_the_archive_command_builds_a_release_for_a_device(self):
        calls = []
        with (
            patch.object(package_ios, "logged", side_effect=lambda command, log, **_: calls.append([str(part) for part in command])),
            patch.object(package_ios.shutil, "rmtree"),
            patch.object(Path, "mkdir"),
            patch.object(package_ios, "archived_app", return_value=Path(__file__).parent),
        ):
            package_ios.archive_app(package_ios.release.current(), package_ios.resolve_signing(FULL), {})
        command = calls[0]
        self.assertEqual(command[0], "xcodebuild")
        self.assertEqual(command[-1], "archive")
        self.assertIn("generic/platform=iOS", command)
        self.assertEqual(command[command.index("-configuration") + 1], "Release")
        self.assertEqual(command[command.index("-scheme") + 1], "SoyehtPhone")


class PreflightTests(unittest.TestCase):
    """The signing is tried on a scratch file before the archive, so a locked keychain fails at once."""

    def preflight(self, *, returncode=0, stderr="", timeout=False):
        calls = []

        def runner(command, **options):
            calls.append((command, options))
            if timeout:
                raise package_ios.subprocess.TimeoutExpired(command, options["timeout"])
            return result(stderr=stderr, returncode=returncode)

        package_ios.signing_preflight(package_ios.resolve_signing(FULL), runner)
        return calls

    def test_a_usable_identity_signs_a_scratch_file_and_nothing_more(self):
        (command, options), = self.preflight()
        self.assertEqual(command[:4], ["codesign", "--force", "--sign", FULL[package_ios.IDENTITY_VARIABLE]])
        self.assertEqual(len(command), 5)
        self.assertFalse(Path(command[4]).exists(), "the scratch file is removed")
        self.assertNotIn("--entitlements", command)
        self.assertIs(options["stdin"], package_ios.subprocess.DEVNULL)  # nothing can be asked of it: no prompt waits

    def test_a_locked_keychain_stops_with_the_cause_and_the_way_out(self):
        for stderr in ("probe: errSecInternalComponent", "User interaction is not allowed."):
            with self.assertRaisesRegex(PackagingError, "keychain is locked.*unlock the login keychain.*cannot prompt"):
                self.preflight(returncode=1, stderr=stderr)

    def test_a_keychain_that_asks_and_never_gets_an_answer_is_the_same_stop(self):
        with self.assertRaisesRegex(PackagingError, "waited.*unlock the login keychain"):
            self.preflight(timeout=True)

    def test_an_identity_the_account_does_not_have_is_named_by_its_variable_not_its_value(self):
        with self.assertRaises(PackagingError) as raised:
            self.preflight(returncode=1, stderr=f"{FULL[package_ios.IDENTITY_VARIABLE]}: no identity found")
        self.assertIn(package_ios.IDENTITY_VARIABLE, str(raised.exception))
        self.assertNotIn("Example", str(raised.exception))

    def test_another_failure_shows_its_output_with_the_identity_masked(self):
        with self.assertRaises(PackagingError) as raised:
            self.preflight(returncode=2, stderr=f"odd failure of {FULL[package_ios.IDENTITY_VARIABLE]}")
        self.assertIn("status 2", str(raised.exception))
        self.assertIn("odd failure", str(raised.exception))
        self.assertNotIn("Example", str(raised.exception))

    def test_the_run_checks_the_keychain_before_it_archives(self):
        order = []
        with (
            patch.object(package_ios, "signing_preflight", side_effect=lambda *_: order.append("preflight")),
            patch.object(package_ios, "archive_app", side_effect=lambda *_: order.append("archive") or 1 / 0),
            patch.object(package_ios.release, "problems", return_value=[]),
            self.assertRaises(ZeroDivisionError),
        ):
            package_ios.package(package_ios.parser().parse_args([]), FULL)
        self.assertEqual(order, ["preflight", "archive"])
        with (
            patch.object(package_ios, "signing_preflight", side_effect=PackagingError("locked")),
            patch.object(package_ios, "archive_app") as archive,
            patch.object(package_ios.release, "problems", return_value=[]),
            self.assertRaisesRegex(PackagingError, "locked"),
        ):
            package_ios.package(package_ios.parser().parse_args([]), FULL)
        archive.assert_not_called()


class LoggedBuildTests(unittest.TestCase):
    """A build's output goes to a file; the terminal gets the command and, on failure, the tail."""

    SCRIPT = "import sys\nfor n in range(300): print('line', n, 'secret-name')\nsys.exit(int(sys.argv[1]))"

    def run_logged(self, status, mask=("secret-name",)):
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / "logs/build.log"
            out = io.StringIO()
            failure = None
            with contextlib.redirect_stdout(out):
                try:
                    package_common.logged([sys.executable, "-c", self.SCRIPT, str(status)], log, mask=mask)
                except PackagingError as raised:
                    failure = str(raised)
            return out.getvalue(), failure, log.read_text()

    def test_a_build_that_passes_prints_its_command_and_log_path_not_its_output(self):
        printed, failure, written = self.run_logged(0)
        self.assertIsNone(failure)
        self.assertNotIn("line 1 ", printed)
        self.assertIn("log:", printed)
        self.assertEqual(len(written.splitlines()), 300)

    def test_a_build_that_fails_shows_only_the_tail_and_where_the_rest_is(self):
        _, failure, written = self.run_logged(3)
        self.assertIn("status 3", failure)
        self.assertIn("line 299", failure)
        self.assertNotIn("line 100 ", failure)
        self.assertEqual(len(failure.splitlines()), package_common.LOG_TAIL_LINES + 1)
        self.assertIn("build.log", failure)

    def test_what_is_masked_reaches_neither_the_file_nor_the_terminal(self):
        printed, failure, written = self.run_logged(3)
        for text in (printed, failure, written):
            self.assertNotIn("secret-name", text)
        self.assertIn(package_common.REDACTED, written)

    def test_the_team_of_the_signing_is_masked_in_the_file_and_in_the_failure_tail(self):
        signing = package_ios.resolve_signing(FULL)
        self.assertIn(TEAM, signing.secrets)
        script = f"import sys\nprint('DEVELOPMENT_TEAM={TEAM}')\nsys.exit(int(sys.argv[1]))"
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / "logs/archive.log"
            with contextlib.redirect_stdout(io.StringIO()):
                package_common.logged([sys.executable, "-c", script, "0"], log, mask=signing.secrets)
            self.assertEqual(log.read_text(), f"DEVELOPMENT_TEAM={package_common.REDACTED}\n")
            with contextlib.redirect_stdout(io.StringIO()), self.assertRaises(PackagingError) as raised:
                package_common.logged([sys.executable, "-c", script, "4"], log, mask=signing.secrets)
        self.assertNotIn(TEAM, str(raised.exception))
        self.assertIn(package_common.REDACTED, str(raised.exception))

    def test_the_log_file_is_readable_by_its_owner_alone(self):
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / "logs/build.log"
            with contextlib.redirect_stdout(io.StringIO()):
                package_common.logged([sys.executable, "-c", "print('built')"], log)
            self.assertEqual(stat.S_IMODE(log.stat().st_mode), 0o600)

    def test_a_byte_that_is_not_utf8_does_not_stop_the_build(self):
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / "logs/build.log"
            write = "import sys\nsys.stdout.buffer.write(b'bad \\xff byte\\n')"
            with contextlib.redirect_stdout(io.StringIO()):
                package_common.logged([sys.executable, "-c", write], log)
            self.assertIn("bad \ufffd byte", log.read_text())


class EntitlementTests(unittest.TestCase):
    SOURCE = {"aps-environment": "development"}

    def test_the_signed_entitlements_hold_the_identity_keys_and_production_push(self):
        entitlements = package_ios.signing_entitlements(self.SOURCE, TEAM)
        self.assertEqual(entitlements["application-identifier"], f"{TEAM}.com.soyeht.next.ios")
        self.assertEqual(entitlements["com.apple.developer.team-identifier"], TEAM)
        self.assertIs(entitlements["get-task-allow"], False)
        self.assertEqual(entitlements["aps-environment"], "production")

    def test_a_file_without_push_gives_an_app_without_it(self):
        self.assertNotIn("aps-environment", package_ios.signing_entitlements({}, TEAM))

    def test_the_source_is_not_changed(self):
        package_ios.signing_entitlements(self.SOURCE, TEAM)
        self.assertEqual(self.SOURCE, {"aps-environment": "development"})

    def test_the_repository_file_is_what_the_script_signs_with(self):
        source = plistlib.loads(package_ios.ENTITLEMENTS.read_bytes())
        self.assertEqual(set(package_ios.signing_entitlements(source, TEAM)) - set(source), set(package_ios.IDENTITY_KEYS))

    def test_the_exported_entitlements_pass_and_each_mistake_is_named(self):
        declared = {"aps-environment"}
        self.assertEqual(package_ios.check_entitlements(GOOD_ENTITLEMENTS, TEAM, declared), [])
        identity_only = {key: GOOD_ENTITLEMENTS[key] for key in package_ios.IDENTITY_KEYS}
        self.assertEqual(package_ios.check_entitlements(identity_only, TEAM, set()), [])
        cases = {
            "get-task-allow": ({**GOOD_ENTITLEMENTS, "get-task-allow": True}, "get-task-allow"),
            "development push": ({**GOOD_ENTITLEMENTS, "aps-environment": "development"}, "aps-environment"),
            "other team": ({**GOOD_ENTITLEMENTS, "com.apple.developer.team-identifier": "ZZZZZ99999"}, "team-identifier"),
            "other app": ({**GOOD_ENTITLEMENTS, "application-identifier": f"{TEAM}.com.example"}, "application-identifier"),
            "extra": ({**GOOD_ENTITLEMENTS, "com.apple.developer.healthkit": True}, "unexpected entitlement"),
            "none": ({}, "application-identifier"),
            "push declared but missing": (identity_only, "aps-environment is missing"),
        }
        for name, (entitled, expected) in cases.items():
            with self.subTest(name):
                self.assertIn(expected, "\n".join(package_ios.check_entitlements(entitled, TEAM, declared)))

    def test_push_the_file_does_not_declare_is_unexpected_in_the_app(self):
        problems = package_ios.check_entitlements(GOOD_ENTITLEMENTS, TEAM, set())
        self.assertEqual(problems, ["unexpected entitlement aps-environment"])

    def test_the_declared_keys_are_those_of_the_repository_file(self):
        self.assertEqual(package_ios.declared_entitlements(), set(plistlib.loads(package_ios.ENTITLEMENTS.read_bytes())))


class ExportTests(unittest.TestCase):
    def test_the_committed_options_name_the_method_and_no_team_or_certificate_identity(self):
        options = plistlib.loads(package_ios.EXPORT_OPTIONS.read_bytes())
        self.assertEqual(options["method"], "app-store-connect")
        self.assertEqual(options["signingStyle"], "manual")
        self.assertNotIn("teamID", options)
        self.assertIn(package_ios.APP_ID, options["provisioningProfiles"])
        self.assertEqual(options["signingCertificate"], "Apple Distribution")
        self.assertIs(options["manageAppVersionAndBuildNumber"], False)

    def test_the_team_and_identity_come_from_the_environment_into_a_copy(self):
        template = package_ios.EXPORT_OPTIONS.read_bytes()
        options = package_ios.export_options(template, package_ios.resolve_signing(FULL))
        self.assertEqual(options["teamID"], TEAM)
        self.assertEqual(options["signingCertificate"], FULL[package_ios.IDENTITY_VARIABLE])
        self.assertEqual(package_ios.EXPORT_OPTIONS.read_bytes(), template)

    def test_a_profile_of_the_environment_replaces_the_committed_one(self):
        signing = package_ios.resolve_signing({**FULL, package_ios.PROFILE_VARIABLE: "Other profile"})
        options = package_ios.export_options(package_ios.EXPORT_OPTIONS.read_bytes(), signing)
        self.assertEqual(options["provisioningProfiles"], {package_ios.APP_ID: "Other profile"})

    def test_options_without_a_profile_for_the_app_are_refused(self):
        template = plistlib.dumps({"method": "app-store-connect", "provisioningProfiles": {"com.example": "x"}})
        with self.assertRaisesRegex(PackagingError, "no profile"):
            package_ios.export_options(template, package_ios.resolve_signing(FULL))

    def test_the_export_runs_exportarchive_and_wants_exactly_one_ipa(self):
        signing = package_ios.resolve_signing(FULL)
        with tempfile.TemporaryDirectory() as temporary:
            scratch = Path(temporary)
            calls = []
            written = {}

            def export(command, log, **_):
                calls.append([str(part) for part in command])
                options = Path(command[command.index("-exportOptionsPlist") + 1])
                written.update(plistlib.loads(options.read_bytes()), options_file=options)
                folder = Path(command[command.index("-exportPath") + 1])
                folder.mkdir(parents=True, exist_ok=True)
                (folder / "Soyeht Next.ipa").write_bytes(b"zip")

            with patch.object(package_ios, "logged", side_effect=export):
                ipa = package_ios.export_archive(signing, scratch)
            self.assertEqual(ipa.name, "Soyeht Next.ipa")
            self.assertEqual(calls[0][:2], ["xcodebuild", "-exportArchive"])
            self.assertEqual(written["teamID"], TEAM)
            self.assertFalse(written["options_file"].exists(), "the options hold the team and the identity")
            self.assertFalse((scratch / "ExportOptions.plist").exists())

            def two(*_a, **_k):
                (scratch / "export").mkdir(exist_ok=True)
                (scratch / "export/Another.ipa").write_bytes(b"zip")
                (scratch / "export/More.ipa").write_bytes(b"zip")

            with patch.object(package_ios, "logged", side_effect=two):
                with self.assertRaisesRegex(PackagingError, "not one"):
                    package_ios.export_archive(signing, scratch)

    def sign(self, app, signing):
        calls = []
        seen = {}

        def record(command, **options):
            calls.append([str(part) for part in command])
            if "--entitlements" in command:
                file = Path(command[command.index("--entitlements") + 1])
                seen.update(plistlib.loads(file.read_bytes()), entitlements_file=file)

        with patch.object(package_ios, "run", side_effect=record):
            package_ios.sign_app(app, signing)
        return calls, seen

    def test_signing_runs_the_nested_code_first_and_then_the_app_with_its_entitlements(self):
        signing = package_ios.resolve_signing(FULL)
        with tempfile.TemporaryDirectory() as temporary:
            app = Path(temporary) / "Soyeht Next.app"
            (app / "Frameworks/Inner.framework").mkdir(parents=True)
            (app / "PlugIns/Ext.appex").mkdir(parents=True)
            calls, seen = self.sign(app, signing)
            self.assertEqual(len(calls), 3)
            self.assertTrue(all(call[:3] == ["codesign", "--force", "--sign"] for call in calls))
            self.assertNotIn("--entitlements", calls[0] + calls[1])
            self.assertIn("--entitlements", calls[2])
            self.assertEqual(calls[2][-1], str(app))
            self.assertEqual(seen["com.apple.developer.team-identifier"], TEAM)
            self.assertFalse(seen["entitlements_file"].exists(), "the entitlements hold the team")

    def test_nested_code_is_signed_deepest_first(self):
        signing = package_ios.resolve_signing(FULL)
        with tempfile.TemporaryDirectory() as temporary:
            app = Path(temporary) / "Soyeht Next.app"
            library = app / "PlugIns/Foo.appex/Frameworks/Bar.framework"
            library.mkdir(parents=True)
            (library / "Baz.dylib").write_bytes(b"x")
            calls, _ = self.sign(app, signing)
        order = [Path(call[-1]).name for call in calls]
        self.assertEqual(order, ["Baz.dylib", "Bar.framework", "Foo.appex", "Soyeht Next.app"])


class RedactionTests(unittest.TestCase):
    KEY = ("/keys/AuthKey_EXAMPLEKEY1.p8", "EXAMPLEKEY1", "11111111-2222-3333-4444-555555555555")

    def printed(self, call):
        output = io.StringIO()
        with (
            patch.object(package_common.subprocess, "run", return_value=result()),
            contextlib.redirect_stdout(output),
        ):
            call()
        return output.getvalue()

    def test_validation_prints_the_command_without_the_key_file_id_or_issuer(self):
        text = self.printed(lambda: package_ios.validate(Path("a.ipa"), self.KEY))
        self.assertTrue(text.startswith("+ xcrun altool --validate-app"), text)
        for secret in self.KEY:
            self.assertNotIn(secret, text)
        self.assertIn(package_common.REDACTED, text)

    def test_signing_prints_the_command_without_the_certificate_name(self):
        signing = package_ios.resolve_signing(FULL)
        with tempfile.TemporaryDirectory() as temporary:
            app = Path(temporary) / "Soyeht Next.app"
            app.mkdir()
            text = self.printed(lambda: package_ios.sign_app(app, signing))
        self.assertIn("+ codesign --force --sign " + package_common.REDACTED, text)
        self.assertNotIn(FULL[package_ios.IDENTITY_VARIABLE], text)
        self.assertNotIn("Example", text)

    def test_the_longer_secret_is_masked_whole_when_one_holds_another(self):
        line = package_common.printable(["x", "Apple Distribution: Example (ABCDE12345)", TEAM], mask=(TEAM, "Apple Distribution: Example (ABCDE12345)"))
        self.assertEqual(line, f"+ x {package_common.REDACTED} {package_common.REDACTED}")

    def test_a_command_with_nothing_to_mask_is_printed_as_it_is(self):
        self.assertEqual(package_common.printable(["a", Path("b")]), "+ a b")

    def test_the_mac_notary_credentials_and_identity_are_masked_too(self):
        import package_mac

        signing = package_mac.Signing(
            "Developer ID Application: Example (ABCDE12345)", TEAM, ("--key", *self.KEY[:1], "--key-id", self.KEY[1], "--issuer", self.KEY[2])
        )
        calls = []
        with (
            patch.object(package_common.subprocess, "run", return_value=result('{"status": "Accepted"}')),
            contextlib.redirect_stdout(io.StringIO()) as output,
        ):
            package_mac.notarize(Path("x.zip"), signing)
            calls.append(output.getvalue())
        text = calls[0]
        self.assertTrue(text.startswith("+ xcrun notarytool submit x.zip"), text)
        for secret in self.KEY:
            self.assertNotIn(secret, text)


class VerifyTests(unittest.TestCase):
    def info(self, **changes):
        made = package_ios.release.current()
        info = {
            "CFBundleIdentifier": package_ios.APP_ID,
            "CFBundleShortVersionString": str(made.version),
            "CFBundleVersion": str(made.build_number()),
        }
        return {**info, **changes}

    def test_the_identifier_version_and_build_number_are_those_of_the_release(self):
        made = package_ios.release.current()
        self.assertEqual(package_common.info_problems(self.info(), package_ios.APP_ID, made), [])
        problems = package_common.info_problems(self.info(CFBundleIdentifier="com.soyeht.next.dev.ios", CFBundleVersion="1"), package_ios.APP_ID, made)
        self.assertEqual(len(problems), 2)

    def test_an_info_plist_that_names_the_legacy_app_is_a_problem(self):
        made = package_ios.release.current()
        problems = package_common.info_problems({**self.info(), "Other": BUNDLE_IDS[0]}, package_ios.APP_ID, made)
        self.assertEqual(problems, ["Info.plist names the legacy app"])

    def test_a_distribution_signature_of_the_team_passes(self):
        self.assertEqual(package_ios.check_signature(GOOD_DETAILS, TEAM), [])

    def test_a_development_certificate_or_another_team_is_a_problem(self):
        development = f"Authority=Apple Development: Example ({TEAM})\nTeamIdentifier={TEAM}\n"
        self.assertIn("Apple Distribution", "\n".join(package_ios.check_signature(development, TEAM)))
        self.assertIn("team is not", "\n".join(package_ios.check_signature(GOOD_DETAILS, "ZZZZZ99999")))
        self.assertTrue(package_ios.check_signature("", TEAM))

    def test_an_app_store_profile_has_no_devices_and_no_debugging(self):
        self.assertEqual(package_ios.check_profile(GOOD_PROFILE, TEAM), [])
        for changes in ({"ProvisionedDevices": ["x"]}, {"ProvisionsAllDevices": True}):
            self.assertIn("App Store profile", "\n".join(package_ios.check_profile({**GOOD_PROFILE, **changes}, TEAM)))
        debuggable = {"Entitlements": {**GOOD_PROFILE["Entitlements"], "get-task-allow": True}}
        self.assertIn("get-task-allow", "\n".join(package_ios.check_profile(debuggable, TEAM)))
        other = {"Entitlements": {"application-identifier": f"{TEAM}.com.example"}}
        self.assertIn("not for", "\n".join(package_ios.check_profile(other, TEAM)))

    def make_ipa_folder(self, root, name="Soyeht Next.app"):
        app = root / "Payload" / name
        app.mkdir(parents=True)
        (app / "Info.plist").write_bytes(plistlib.dumps(self.info()))
        return app

    def test_an_exported_app_is_checked_on_every_count(self):
        made = package_ios.release.current()
        with tempfile.TemporaryDirectory() as temporary:
            app = self.make_ipa_folder(Path(temporary))
            with (
                patch.object(package_ios, "run", return_value=result()),
                patch.object(package_ios, "signature_details", return_value=GOOD_DETAILS),
                patch.object(package_ios, "entitlement_values", return_value=GOOD_ENTITLEMENTS),
                patch.object(package_ios, "embedded_profile", return_value=GOOD_PROFILE),
            ):
                self.assertEqual(package_ios.verify_app(app, TEAM, made), [])
            with (
                patch.object(package_ios, "run", return_value=result()),
                patch.object(package_ios, "signature_details", return_value=""),
                patch.object(package_ios, "entitlement_values", return_value={**GOOD_ENTITLEMENTS, "get-task-allow": True}),
                patch.object(package_ios, "embedded_profile", return_value={**GOOD_PROFILE, "ProvisionedDevices": ["x"]}),
            ):
                problems = package_ios.verify_app(app, TEAM, made)
            self.assertEqual(len(problems), 4, problems)

    def test_an_app_without_its_profile_stops_the_check(self):
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(PackagingError, "embedded.mobileprovision"):
                package_ios.embedded_profile(Path(temporary))

    def test_the_ipa_holds_one_app_called_as_the_product(self):
        made = package_ios.release.current()
        for name, expected in (("Other.app", "not Soyeht Next.app"), ("Soyeht Next.app", None)):
            with self.subTest(name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary) / "unpacked"
                self.make_ipa_folder(root, name)
                with (
                    patch.object(package_ios.tempfile, "TemporaryDirectory", return_value=contextlib.nullcontext(str(root))),
                    patch.object(package_ios, "run", return_value=result()),
                    patch.object(package_ios, "verify_app", return_value=[]),
                ):
                    problems = package_ios.verify_ipa(Path("x.ipa"), TEAM, made)
                self.assertEqual(any(expected in problem for problem in problems) if expected else problems, True if expected else [])

    def test_an_ipa_with_no_app_is_a_problem(self):
        with tempfile.TemporaryDirectory() as temporary:
            (Path(temporary) / "Payload").mkdir()
            with (
                patch.object(package_ios.tempfile, "TemporaryDirectory", return_value=contextlib.nullcontext(temporary)),
                patch.object(package_ios, "run", return_value=result()),
            ):
                problems = package_ios.verify_ipa(Path("x.ipa"), TEAM, package_ios.release.current())
            self.assertIn("not one", problems[0])

    def test_verify_only_needs_the_team_and_the_file(self):
        with self.assertRaisesRegex(PackagingError, package_common.TEAM_VARIABLE):
            package_ios.package(package_ios.parser().parse_args(["--verify-only", "x.ipa"]), {})
        with self.assertRaisesRegex(PackagingError, "not a file"):
            package_ios.package(package_ios.parser().parse_args(["--verify-only", "/nonexistent/x.ipa"]), {package_common.TEAM_VARIABLE: TEAM})

    def test_verify_only_reports_the_problems_and_the_status(self):
        with tempfile.TemporaryDirectory() as temporary:
            ipa = Path(temporary) / "x.ipa"
            ipa.write_bytes(b"zip")
            arguments = package_ios.parser().parse_args(["--verify-only", str(ipa)])
            for problems, status in (([], 0), (["a", "b"], 1)):
                with patch.object(package_ios, "verify_ipa", return_value=problems), contextlib.redirect_stderr(io.StringIO()):
                    with contextlib.redirect_stdout(io.StringIO()):
                        self.assertEqual(package_ios.package(arguments, {package_common.TEAM_VARIABLE: TEAM}), status)


class ValidateTests(unittest.TestCase):
    def api_key(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        key = Path(folder.name) / "AuthKey_EXAMPLE.p8"
        key.write_text("not a real key")
        key.chmod(0o600)
        return {
            package_common.NOTARY_KEY_FILE_VARIABLE: str(key),
            package_common.NOTARY_KEY_ID_VARIABLE: "EXAMPLEKEY1",
            package_common.NOTARY_ISSUER_VARIABLE: "00000000-0000-0000-0000-000000000000",
        }

    def test_validation_asks_for_the_file_to_be_checked_and_never_uploads(self):
        environment = self.api_key()
        command = [str(part) for part in package_ios.validate_command(Path("a.ipa"), package_common.api_key(environment))]
        self.assertEqual(command[:4], ["xcrun", "altool", "--validate-app", "a.ipa"])
        self.assertNotIn("--type", command)
        self.assertNotIn("--file", command)
        self.assertNotIn("--upload-app", command)
        self.assertEqual(command[command.index("--api-key") + 1], "EXAMPLEKEY1")
        self.assertEqual(command[command.index("--p8-file-path") + 1], environment[package_common.NOTARY_KEY_FILE_VARIABLE])

    def test_validate_without_the_key_variables_stops_before_anything_is_built(self):
        arguments = package_ios.parser().parse_args(["--validate"])
        with patch.object(package_ios, "archive_app") as archive:
            with self.assertRaisesRegex(PackagingError, "--validate needs"):
                package_ios.package(arguments, FULL)
        archive.assert_not_called()

    def test_a_half_set_key_is_an_error_too(self):
        environment = {**FULL, package_common.NOTARY_KEY_ID_VARIABLE: "EXAMPLEKEY1"}
        with self.assertRaisesRegex(PackagingError, package_common.NOTARY_KEY_FILE_VARIABLE):
            package_ios.package(package_ios.parser().parse_args(["--validate"]), environment)

    def test_a_run_without_validate_never_reads_or_runs_the_validation(self):
        arguments = package_ios.parser().parse_args([])
        self.assertFalse(arguments.validate)

    def run_package(self, arguments, environment, problems):
        with tempfile.TemporaryDirectory() as temporary:
            ipa = Path(temporary) / "export.ipa"
            ipa.write_bytes(b"zip")
            with (
                patch.object(package_ios, "archive_app", return_value=Path(temporary)),
                patch.object(package_ios, "signing_preflight"),
                patch.object(package_ios, "sign_app"),
                patch.object(package_ios, "export_archive", return_value=ipa),
                patch.object(package_ios, "save_symbols", return_value=None),
                patch.object(package_ios, "verify_ipa", return_value=problems),
                patch.object(package_ios, "check_sizes"),
                patch.object(package_ios.release, "problems", return_value=[]),
                patch.object(package_ios, "validate") as validate,
                contextlib.redirect_stdout(io.StringIO()),
                contextlib.redirect_stderr(io.StringIO()),
            ):
                status = package_ios.package(package_ios.parser().parse_args([*arguments, "--dist", temporary]), environment)
                return status, validate, (Path(temporary) / f"soyeht-{package_ios.release.current().version}-ios-aarch64.ipa").is_file()

    def test_a_whole_run_validates_only_when_asked_and_only_when_the_checks_pass(self):
        key = self.api_key()
        status, validate, copied = self.run_package([], FULL, [])
        self.assertEqual((status, copied), (0, True))
        validate.assert_not_called()
        status, validate, _ = self.run_package(["--validate"], {**FULL, **key}, [])
        self.assertEqual(status, 0)
        validate.assert_called_once()
        status, validate, _ = self.run_package(["--validate"], {**FULL, **key}, ["a problem"])
        self.assertEqual(status, 1)
        validate.assert_not_called()


class NameTests(unittest.TestCase):
    def test_the_ipa_is_named_like_the_other_packages_and_has_a_size_line(self):
        made = package_ios.release.current()
        path = package_common.artifact_path(Path("dist/ios"), made.version, "ios", "aarch64", "ipa")
        self.assertEqual(path.name, f"soyeht-{made.version}-ios-aarch64.ipa")
        kind, _ = package_common.size_ceiling(path, package_common.json.loads(package_common.SIZE_CEILINGS.read_text())["ceilings"])
        self.assertEqual(kind, "ios-aarch64.ipa")

    def test_the_app_name_is_the_one_of_the_production_profile(self):
        self.assertEqual(package_ios.PRODUCT_NAME, "Soyeht Next")


if __name__ == "__main__":
    unittest.main()
