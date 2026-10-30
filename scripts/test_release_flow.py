import base64
import contextlib
import hashlib
import io
import json
import struct
import tempfile
import time
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

import changelog
import release
import package_common
import release_checks
import release_flow
import release_manifest
import update_manifest as manifest
import ed25519

SEED = hashlib.sha256(b"soyeht release flow test key").digest()
KEYS = [{"id": "test-key", "key": base64.b64encode(ed25519.public_key(SEED)).decode()}]
SECRET = base64.b64encode(SEED).decode()
ENVIRONMENT = {release_manifest.KEY_ENVIRONMENT: SECRET}
VERSION = str(release.current().version)
SHA = "a" * 40
NOTES = f"## [{VERSION}] - 2026-11-01\n\n### Added\n\n- A thing a person can do.\n\n"
DRAFT = f"## [{VERSION}] - 2026-11-01\n\n<!-- draft: generated -->\n\n### Added\n\n- A thing.\n\n"


def pe(signed):
    """The few header bytes of a PE32+ file that the signature check reads."""
    data = bytearray(0x400)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<H", data, 0x80 + 24, 0x20B)
    if signed:
        struct.pack_into("<II", data, 0x80 + 24 + 112 + 8 * 4, 0x300, 0x40)
    return bytes(data)


def process(returncode=0, stderr=""):
    return SimpleNamespace(returncode=returncode, stderr=stderr, stdout="")


def developer_id_tool(command):
    return process(stderr=release_checks.DEVELOPER_ID + " Example (ABCDE12345)\n") if command[0] == "codesign" else process()


def ad_hoc_tool(command):
    return process(1 if command[0] != "codesign" else 0, "Signature=adhoc\n")


class Recorder:
    """Stands in for `Runner`: keeps the commands and runs nothing."""

    def __init__(self):
        self.commands = []
        self.inputs = []

    def must(self, command, stdin=None, **_):
        self.commands.append([str(part) for part in command])
        self.inputs.append(stdin)


def probe_for(release_exists=False, dirty=False, on_main=True, gh_fails=False):
    def probe(command):
        if command[:2] == ["git", "status"]:
            return 0, " M file\n" if dirty else ""
        if command[:2] == ["git", "merge-base"]:
            return (0 if on_main else 1), ""
        if command[:2] == ["git", "rev-parse"]:
            return 0, SHA + "\n"
        if command[:3] == ["gh", "release", "list"]:
            if gh_fails:
                return 1, ""
            return 0, json.dumps([{"tagName": f"v{VERSION}", "isDraft": True}] if release_exists else [{"tagName": "v0.0.1", "isDraft": False}])
        return 0, ""

    return probe


class FlowCase(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.dist = self.root / "dist"
        self.dist.mkdir()
        self.write_changelog(NOTES)
        for patch in (
            mock.patch.dict(release_manifest.POLICY["signature"], {"publicKeys": KEYS}),
            mock.patch.object(changelog, "CHANGELOG", self.root / "CHANGELOG.md"),
        ):
            patch.start()
            self.addCleanup(patch.stop)

    def write_changelog(self, text):
        (self.root / "CHANGELOG.md").write_text("# Changelog\n\n## [Unreleased]\n\n" + text)

    def package(self, system, architecture, extension, content=None):
        name = release_checks.package_name(VERSION, system, architecture, extension)
        (self.dist / name).write_bytes(content or f"{system}{architecture}{extension}".encode())
        return name

    def all_packages(self, signed=True):
        (self.dist / release_checks.BUILD_SHA).write_text(SHA + "\n")
        self.package("macos", "aarch64", "dmg")
        self.package("linux", "x86_64", "deb")
        self.package("linux", "x86_64", "AppImage")
        self.package("windows", "x86_64", "exe", pe(signed))
        self.package("windows", "aarch64", "exe", pe(signed))

    def options(self, *argv):
        return release_flow.parser().parse_args([*argv, "--dist", str(self.dist)])

    def collect(self, *argv, environment=ENVIRONMENT, fetch=lambda _address: None, tool=developer_id_tool):
        return release_flow.collect(self.options("collect", *argv), environment, fetch, tool, probe_for())

    def quiet(self, function, *args, **kwargs):
        with contextlib.redirect_stdout(io.StringIO()) as out:
            result = function(*args, **kwargs)
        return result, out.getvalue()


class CollectTests(FlowCase):
    def test_collect_writes_the_checksums_the_notes_and_a_manifest_a_client_accepts(self):
        self.all_packages()
        self.quiet(self.collect)
        names = release_flow.package_names(self.dist, VERSION)
        assets = [*names, release_manifest.POLICY["manifest"]["asset"]]
        self.assertEqual(release_checks.checksum_problems(self.dist, assets), [])
        self.assertEqual((self.dist / release_checks.NOTES).read_text(), NOTES.split("\n", 1)[1].strip() + "\n")
        text = (self.dist / "update-manifest.json").read_text()
        offer = manifest.evaluate(text, (0, 0, 1), "linux", "x86_64", "stable", KEYS)
        self.assertTrue(offer["package"]["url"].endswith(".AppImage"))
        self.assertEqual(manifest.evaluate(text, (0, 0, 1), "windows", "aarch64", "stable", KEYS)["status"], "available")

    def test_a_package_changed_after_collect_no_longer_matches_the_checksums(self):
        self.all_packages()
        self.quiet(self.collect)
        (self.dist / release_checks.package_name(VERSION, "linux", "x86_64", "deb")).write_bytes(b"changed")
        names = release_flow.package_names(self.dist, VERSION)
        self.assertTrue(any("deb" in line for line in release_checks.checksum_problems(self.dist, names)))

    def test_every_missing_or_unsigned_part_is_listed_at_once(self):
        self.package("macos", "aarch64", "dmg")
        self.package("windows", "x86_64", "exe", pe(False))
        with self.assertRaises(release_flow.Refused) as caught:
            self.quiet(self.collect, tool=ad_hoc_tool)
        text = str(caught.exception)
        for part in ("-linux-x86_64.deb is missing", "-linux-x86_64.AppImage is missing", "-windows-aarch64.exe is missing",
                     "not signed with a Developer ID", "no stapled notarization ticket", "rejected by Gatekeeper",
                     "carries no Authenticode signature"):
            self.assertIn(part, text)

    def test_no_update_key_stops_a_release_but_not_a_pre_release(self):
        self.all_packages()
        with self.assertRaises(release_flow.Refused) as caught:
            self.quiet(self.collect, environment={})
        self.assertIn(release_manifest.KEY_ENVIRONMENT, str(caught.exception))
        self.quiet(self.collect, "--allow-unsigned", environment={}, tool=ad_hoc_tool)
        self.assertFalse((self.dist / "update-manifest.json").exists())
        self.assertTrue((self.dist / release_checks.CHECKSUMS).is_file())

    def test_a_pre_release_takes_the_ad_hoc_image_and_leaves_a_system_out(self):
        (self.dist / release_checks.BUILD_SHA).write_text(SHA + "\n")
        (self.dist / "ad-hoc").mkdir()
        (self.dist / "ad-hoc" / release_checks.BUILD_SHA).write_text(SHA + "\n")
        (self.dist / "ad-hoc" / release_checks.package_name(VERSION, "macos", "aarch64", "dmg")).write_bytes(b"adhoc")
        self.package("linux", "x86_64", "deb")
        self.package("linux", "x86_64", "AppImage")
        with self.assertRaises(release_flow.Refused):
            self.quiet(self.collect, "--without", "windows")  # not allowed for a release
        self.quiet(self.collect, "--allow-unsigned", "--without", "windows", tool=ad_hoc_tool)
        self.assertTrue((self.dist / release_checks.package_name(VERSION, "macos", "aarch64", "dmg")).is_file())

    def test_the_other_machines_packages_are_copied_in_and_another_version_is_refused(self):
        self.all_packages()
        windows = self.root / "windows"
        windows.mkdir()
        for arch in ("x86_64", "aarch64"):
            name = release_checks.package_name(VERSION, "windows", arch, "exe")
            (self.dist / name).unlink()
            (windows / name).write_bytes(pe(True))
        (windows / "soyeht-0.0.9-windows-x86_64.exe").write_bytes(pe(True))
        (windows / release_checks.BUILD_SHA).write_text(SHA + "\n")
        with self.assertRaises(release_flow.Refused):
            self.quiet(self.collect, "--from", str(windows))
        (windows / "soyeht-0.0.9-windows-x86_64.exe").unlink()
        _, out = self.quiet(self.collect, "--from", str(windows))
        self.assertIn("copied soyeht-", out)
        self.assertEqual(release_checks.missing_packages(self.dist, VERSION), [])

    def test_packages_from_another_commit_or_without_a_commit_are_refused(self):
        self.all_packages()
        windows = self.root / "windows"
        windows.mkdir()
        (windows / release_checks.package_name(VERSION, "windows", "x86_64", "exe")).write_bytes(pe(True))
        with self.assertRaisesRegex(release_flow.Refused, "no BUILD_SHA"):
            self.quiet(self.collect, "--from", str(windows))
        (windows / release_checks.BUILD_SHA).write_text("b" * 40 + "\n")
        with self.assertRaisesRegex(release_flow.Refused, "built from bbbbbbbbb"):
            self.quiet(self.collect, "--from", str(windows))
        (self.dist / release_checks.BUILD_SHA).write_text("c" * 40 + "\n")
        with self.assertRaisesRegex(release_flow.Refused, "built from cccccccc"):
            self.quiet(self.collect)

    def test_the_marker_is_read_from_what_windows_powershell_writes(self):
        windows = self.root / "windows"
        windows.mkdir()
        marker = windows / release_checks.BUILD_SHA
        for data in (SHA.encode() + b"\n", b"\xef\xbb\xbf" + SHA.encode() + b"\r\n", (SHA + "\r\n").encode("utf-16")):
            marker.write_bytes(data)
            self.assertEqual(release_checks.read_build_sha(marker), SHA)
        for data in (b"\xff\xfe\x00", b"\xc3\x28", b"abc\n", b""):
            marker.write_bytes(data)
            with self.assertRaises(release_checks.MarkerError):
                release_checks.read_build_sha(marker)
        self.all_packages()
        (windows / release_checks.package_name(VERSION, "windows", "x86_64", "exe")).write_bytes(pe(True))
        marker.write_bytes((SHA + "\r\n").encode("utf-16"))
        self.quiet(release_flow.gather, self.dist, VERSION, [windows])
        marker.write_bytes(b"\xc3\x28")
        with self.assertRaisesRegex(release_flow.Refused, "not a commit id"):
            release_flow.gather(self.dist, VERSION, [windows])
        (self.dist / release_checks.BUILD_SHA).write_bytes(b"\xff\xfe\x00")
        self.assertIn("build again", release_flow.build_sha_problems(self.dist, probe_for())[0])

    def test_an_ad_hoc_folder_is_tied_to_the_commit_like_any_other(self):
        self.all_packages()
        adhoc = self.dist / "ad-hoc"
        adhoc.mkdir()
        (adhoc / release_checks.package_name(VERSION, "macos", "aarch64", "dmg")).write_bytes(b"adhoc")
        with self.assertRaisesRegex(release_flow.Refused, "ad-hoc has no BUILD_SHA"):
            self.quiet(self.collect, "--allow-unsigned", tool=ad_hoc_tool)
        (adhoc / release_checks.BUILD_SHA).write_text("f" * 40 + "\n")
        with self.assertRaisesRegex(release_flow.Refused, "built from fffffffff"):
            self.quiet(self.collect, "--allow-unsigned", tool=ad_hoc_tool)
        # A folder named ad-hoc anywhere else is not exempt either.
        elsewhere = self.root / "elsewhere" / "ad-hoc"
        elsewhere.mkdir(parents=True)
        (elsewhere / release_checks.package_name(VERSION, "linux", "x86_64", "deb")).write_bytes(b"x")
        with self.assertRaisesRegex(release_flow.Refused, "has no BUILD_SHA"):
            release_flow.gather(self.dist, VERSION, [elsewhere])

    def test_a_package_of_another_version_is_never_collected_or_uploaded(self):
        self.all_packages()
        (self.dist / "soyeht-0.0.9-macos-aarch64.dmg").write_bytes(b"old")
        self.assertNotIn("soyeht-0.0.9-macos-aarch64.dmg", release_flow.package_names(self.dist, VERSION))
        self.assertEqual([p.name for p in release_flow.publish_files(self.dist, VERSION)].count("soyeht-0.0.9-macos-aarch64.dmg"), 0)
        with self.assertRaisesRegex(release_flow.Refused, "another version than"):
            self.quiet(self.collect)

    def test_a_draft_changelog_stops_the_release(self):
        self.all_packages()
        self.write_changelog(DRAFT)
        with self.assertRaises(release_flow.Refused) as caught:
            self.quiet(self.collect)
        self.assertIn("still a draft", str(caught.exception))

    def test_the_sequence_must_be_above_the_one_the_clients_read_now(self):
        self.all_packages()
        sequence = int(time.time()) + 1000
        old, _ = release_manifest.build(self.dist, ENVIRONMENT, "0.0.0", sequence=sequence)
        with self.assertRaises(release_flow.Refused) as caught:
            self.quiet(self.collect, fetch=lambda _address: old)
        self.assertIn(f"--sequence {sequence + 1}", str(caught.exception))
        self.quiet(self.collect, "--last-sequence", str(sequence - 2000))

    def test_the_way_out_of_a_manifest_published_too_high_is_a_sequence_just_above_it(self):
        self.all_packages()
        bad = int(time.time()) + 3 * 86400
        with self.assertRaises(release_flow.Refused):
            self.quiet(self.collect, "--last-sequence", str(bad))
        self.quiet(self.collect, "--last-sequence", str(bad), "--sequence", str(bad + 1))
        text = (self.dist / "update-manifest.json").read_text()
        self.assertEqual(release_manifest.previous_sequence(text, KEYS), bad + 1)

    def test_the_last_published_manifest_is_read_from_the_address_of_the_contract(self):
        asked = []
        options = self.options("collect")
        self.assertIsNone(release_flow.last_published_sequence(options, lambda address: asked.append(address)))
        self.assertEqual(asked, [release_manifest.POLICY["manifest"]["url"]])
        self.assertIsNone(release_flow.last_published_sequence(self.options("collect", "--first-release"), self.fail))
        self.assertEqual(release_flow.last_published_sequence(self.options("collect", "--last-sequence", "9"), self.fail), 9)


class PublishTests(FlowCase):
    def ready(self):
        self.all_packages()
        self.quiet(self.collect)

    def publish(self, *argv, probe=None, fetch=lambda _address: None):
        recorder = Recorder()
        status, out = self.quiet(
            release_flow.publish, self.options("publish", *argv), recorder, probe or probe_for(), fetch, developer_id_tool
        )
        return status, recorder, out

    def test_the_release_is_a_draft_of_the_public_repository_with_the_manifest_last(self):
        self.ready()
        status, recorder, out = self.publish()
        self.assertEqual(status, 0)
        (command,) = recorder.commands
        self.assertEqual(command[:4], ["gh", "release", "create", f"v{VERSION}"])
        self.assertIn("--draft", command)
        self.assertNotIn("--prerelease", command)
        self.assertEqual(command[command.index("--repo") + 1], "soyeht/soyeht")
        self.assertEqual(command[command.index("--target") + 1], SHA)
        self.assertEqual(command[command.index("--title") + 1], f"Soyeht {VERSION}")
        self.assertEqual(Path(command[command.index("--notes-file") + 1]).name, release_checks.NOTES)
        assets = [Path(part).name for part in command if str(self.dist) in part and not part.endswith(".md")]
        self.assertEqual(assets[-2:], [release_checks.CHECKSUMS, "update-manifest.json"])
        self.assertEqual(len(assets), 7)
        self.assertIn("--draft=false", out)

    def test_a_pre_release_is_marked_and_going_live_is_explicit(self):
        self.ready()
        _, recorder, _ = self.publish("--allow-unsigned")
        self.assertIn("--prerelease", recorder.commands[0])
        _, recorder, out = self.publish("--no-draft")
        self.assertNotIn("--draft", recorder.commands[0])
        self.assertIn("is public", out)

    def test_nothing_goes_out_when_a_part_is_wrong_and_every_reason_is_listed(self):
        self.ready()
        (self.dist / release_checks.package_name(VERSION, "windows", "x86_64", "exe")).write_bytes(pe(False))
        self.write_changelog(DRAFT)
        with self.assertRaises(release_flow.Refused) as caught:
            self.publish(probe=probe_for(release_exists=True, dirty=True, on_main=False))
        text = str(caught.exception)
        for part in ("working tree has changes", "not on origin/main", "does not match SHA256SUMS", "still a draft", "(a draft counts)"):
            self.assertIn(part, text)

    def test_a_manifest_that_lists_other_bytes_than_the_files_is_refused(self):
        self.ready()
        other = self.dist / release_checks.package_name(VERSION, "macos", "aarch64", "dmg")
        other.write_bytes(b"swapped")
        release_checks.write_checksums(self.dist, [*release_flow.package_names(self.dist, VERSION), "update-manifest.json"])
        with self.assertRaises(release_flow.Refused) as caught:
            self.publish()
        self.assertIn("hash or size that is not the file's", str(caught.exception))

    def test_a_manifest_below_the_last_published_is_refused(self):
        self.ready()
        text = (self.dist / "update-manifest.json").read_text()
        sequence = release_manifest.previous_sequence(text, KEYS)
        with self.assertRaises(release_flow.Refused) as caught:
            self.publish("--last-sequence", str(sequence))
        self.assertIn("not above", str(caught.exception))

    def test_the_packages_must_come_from_the_commit_that_is_tagged(self):
        self.ready()
        (self.dist / release_checks.BUILD_SHA).write_text("d" * 40 + "\n")
        with self.assertRaisesRegex(release_flow.Refused, "built from ddddddddd, not from aaaaaaaaa"):
            self.publish()
        (self.dist / release_checks.BUILD_SHA).unlink()
        with self.assertRaisesRegex(release_flow.Refused, "BUILD_SHA is missing"):
            self.publish()

    def test_a_leftover_draft_is_found_and_so_is_a_gh_that_cannot_list(self):
        self.ready()
        with self.assertRaisesRegex(release_flow.Refused, "a draft counts"):
            self.publish(probe=probe_for(release_exists=True))
        with self.assertRaisesRegex(release_flow.Refused, "cannot list the releases of soyeht/soyeht"):
            self.publish(probe=probe_for(gh_fails=True))
        asked = []
        release_flow.existing_release_problems(f"v{VERSION}", lambda command: asked.append(command) or (0, "[]"))
        self.assertEqual(asked[0][:3], ["gh", "release", "list"])
        self.assertIn("tagName,isDraft", asked[0])

    def test_a_missing_release_folder_is_a_list_of_reasons_not_a_crash(self):
        found = release_flow.publish_problems(
            self.options("publish", "--dist", str(self.root / "nowhere")), [], probe_for(), lambda _a: None, developer_id_tool
        )
        self.assertTrue(any("is missing: run collect" in line for line in found))

    def test_a_dry_run_prints_the_command_and_the_reasons_and_changes_nothing(self):
        self.ready()
        (self.dist / release_checks.NOTES).unlink()
        with mock.patch("subprocess.run", side_effect=AssertionError("a dry run ran a command")):
            status, out = self.quiet(release_flow.publish, self.options("publish", "--dry-run"), None, probe_for(), lambda _a: None, developer_id_tool)
        self.assertEqual(status, 1)
        self.assertIn("would refuse", out)
        self.assertIn("+ gh release create", out)
        self.assertIn("(dry run: not run)", out)

    def test_no_command_the_flow_prints_holds_the_secret_or_creates_a_tag(self):
        self.ready()
        _, recorder, out = self.publish()
        build_recorder = Recorder()
        self.quiet(release_flow.build, self.options("build", "--dry-run"), build_recorder, probe_for(), ENVIRONMENT)
        for command in [*recorder.commands, *build_recorder.commands]:
            self.assertNotIn(SECRET, " ".join(command))
            self.assertNotEqual(command[:2], ["git", "tag"])
        self.assertNotIn(SECRET, out)
        self.assertNotIn("--tag", " ".join(recorder.commands[0]))


class BuildTests(FlowCase):
    def build(self, *argv, probe=None, environment=None):
        recorder = Recorder()
        status, out = self.quiet(release_flow.build, self.options("build", *argv), recorder, probe or probe_for(), environment or {})
        return status, recorder, out

    def test_the_mac_package_needs_the_release_flag_unless_it_is_a_pre_release(self):
        _, recorder, _ = self.build("--mac")
        self.assertEqual(recorder.commands[0][:6], ["nice", "-n", "15", "uv", "run", "python"])
        self.assertIn("--release", recorder.commands[0])
        _, recorder, _ = self.build("--mac", "--allow-unsigned")
        self.assertNotIn("--release", recorder.commands[0])

    def test_the_linux_host_gets_the_commit_and_builds_only_inside_soyeht_ci(self):
        status, recorder, _ = self.build("--linux", environment={release_flow.RECEIVE_PACK_VARIABLE: "/nix/git-receive-pack"})
        self.assertEqual(status, 0)
        push, ssh, scp = recorder.commands
        self.assertEqual(push, ["git", "push", "--receive-pack=/nix/git-receive-pack", "bignix:soyeht-ci/mirror.git",
                                f"{SHA}:refs/heads/release-{VERSION}"])
        self.assertEqual(ssh[:4], ["ssh", "-o", "BatchMode=yes", "bignix"])
        script = recorder.inputs[1]
        self.assertIn(f'"$HOME/soyeht-ci/release/{SHA}"', script)
        self.assertIn("package_linux_rootless.sh --format all", script)
        for line in script.splitlines():
            for place in ("cd ", "worktree add"):
                if place in line:
                    self.assertTrue("$HOME/soyeht-ci" in line or '"$tree"' in line, line)
        self.assertEqual(scp[3], f"bignix:soyeht-ci/release/{SHA}/dist/soyeht-{VERSION}-linux-*")

    def test_the_host_comes_from_the_option_or_the_environment_and_must_be_a_name(self):
        _, recorder, _ = self.build("--linux", environment={release_flow.LINUX_HOST_VARIABLE: "ci-box"})
        self.assertTrue(recorder.commands[0][2].startswith("ci-box:"))
        _, recorder, _ = self.build("--linux", "--linux-host", "other")
        self.assertTrue(recorder.commands[0][2].startswith("other:"))
        with self.assertRaises(release_flow.Refused):
            self.build("--linux", "--linux-host", "x; rm -rf ~")
        for name in ("-oProxyCommand=x", "--upload-pack=x", "-"):
            with self.assertRaises(release_flow.Refused):
                self.build("--linux", f"--linux-host={name}")
        with self.assertRaises(release_flow.Refused):
            release_flow.linux_script(VERSION, "../../etc")

    def test_build_records_its_commit_and_a_dry_run_creates_nothing(self):
        fresh = self.root / "fresh"
        options = release_flow.parser().parse_args(["build", "--mac", "--dist", str(fresh)])
        self.quiet(release_flow.build, options, Recorder(), probe_for(), {})
        self.assertEqual((fresh / release_checks.BUILD_SHA).read_text().strip(), SHA)
        dry = self.root / "dry"
        options = release_flow.parser().parse_args(["build", "--mac", "--dry-run", "--dist", str(dry)])
        _, out = self.quiet(release_flow.build, options, Recorder(), probe_for(), {})
        self.assertFalse(dry.exists())
        self.assertIn("would write", out)

    def test_the_commit_is_recorded_after_the_packages_are_built_not_before(self):
        seen = []

        class Watching(Recorder):
            def must(inner, command, **options):
                seen.append((tuple(command[:2]), (self.dist / release_checks.BUILD_SHA).exists()))
                super().must(command, **options)

        self.quiet(release_flow.build, self.options("build", "--mac", "--linux"), Watching(), probe_for(), {})
        self.assertTrue(seen and all(not existed for _, existed in seen))
        self.assertTrue((self.dist / release_checks.BUILD_SHA).is_file())

    def test_a_build_also_marks_an_ad_hoc_folder_and_refuses_one_of_another_commit(self):
        adhoc = self.dist / "ad-hoc"
        adhoc.mkdir()
        (adhoc / release_checks.package_name(VERSION, "macos", "aarch64", "dmg")).write_bytes(b"old")
        with self.assertRaisesRegex(release_flow.Refused, "ad-hoc holds packages with no BUILD_SHA"):
            self.build("--linux")
        (adhoc / release_checks.BUILD_SHA).write_text(SHA + "\n")
        self.build("--linux")
        self.assertEqual((adhoc / release_checks.BUILD_SHA).read_text().strip(), SHA)

    def test_packages_left_by_a_build_of_another_commit_stop_a_new_build(self):
        self.all_packages()
        (self.dist / release_checks.BUILD_SHA).write_text("e" * 40 + "\n")
        with self.assertRaisesRegex(release_flow.Refused, "built from another commit"):
            self.build("--mac")
        (self.dist / release_checks.BUILD_SHA).write_text(SHA + "\n")
        self.build("--mac")

    def test_the_windows_command_is_printed_not_run(self):
        _, recorder, out = self.build("--windows")
        self.assertEqual(recorder.commands, [])
        self.assertIn("package_windows.py --require-signature", out)
        self.assertIn("| Out-File -Encoding ascii dist\\BUILD_SHA", out)
        self.assertIn(f"git checkout {SHA}", out)

    def test_a_release_checkout_is_required_and_a_dry_run_only_reports_it(self):
        with self.assertRaises(release_flow.Refused):
            self.build("--mac", probe=probe_for(dirty=True))
        status, recorder, out = self.build("--mac", "--dry-run", probe=probe_for(dirty=True))
        self.assertEqual(status, 1)
        self.assertIn("would refuse", out)
        self.assertEqual(len(recorder.commands), 1)


class SmallPartTests(FlowCase):
    def test_runner_prints_and_in_a_dry_run_runs_nothing(self):
        with mock.patch("subprocess.run", side_effect=AssertionError("ran")):
            status, out = self.quiet(release_flow.Runner(True).run, ["gh", "release", "create", "v1 x"])
        self.assertEqual(status, 0)
        self.assertIn("+ gh release create 'v1 x'", out)

    def test_an_authenticode_signature_is_the_certificate_table_of_the_header(self):
        path = self.root / "a.exe"
        path.write_bytes(pe(True))
        self.assertTrue(release_checks.pe_is_signed(path))
        path.write_bytes(pe(False))
        self.assertFalse(release_checks.pe_is_signed(path))
        path.write_bytes(b"not a program")
        self.assertFalse(release_checks.pe_is_signed(path))

    def test_the_mac_image_cannot_be_checked_off_the_mac(self):
        self.all_packages()
        found = release_checks.mac_problems(self.dist, release_flow.package_names(self.dist, VERSION), system="linux")
        self.assertEqual(len(found), 1)
        self.assertIn("collect on the Mac", found[0])

    def test_a_damaged_copy_is_found_by_the_checksum_a_packager_wrote(self):
        name = self.package("macos", "aarch64", "dmg", b"image")
        (self.dist / (name + ".sha256")).write_text(f"{release_checks.sha256_of(self.dist / name)}  {name}\n")
        self.assertEqual(release_checks.sidecar_problems(self.dist), [])
        (self.dist / name).write_bytes(b"damaged")
        self.assertEqual(len(release_checks.sidecar_problems(self.dist)), 1)

    def test_the_repository_is_one_name_in_the_contract_the_homepage_and_the_release_address(self):
        self.assertEqual(release_flow.REPOSITORY, "soyeht/soyeht")
        self.assertEqual(package_common.HOMEPAGE, "https://github.com/" + release_flow.REPOSITORY)
        for address in (release_manifest.POLICY["manifest"]["url"], release_manifest.POLICY["release"]["packageUrl"],
                        release_manifest.POLICY["release"]["notesUrl"], *release_manifest.POLICY["urlPrefixes"]):
            self.assertTrue(address.startswith(package_common.HOMEPAGE + "/"), address)

    def test_prepare_bumps_then_drafts_the_target_version_and_a_dry_run_writes_nothing(self):
        current = release.current()
        target = str(release.next_release(current, "minor").version)
        with mock.patch.object(release, "main", return_value=0) as bump, mock.patch.object(changelog, "main", return_value=0) as draft:
            self.quiet(release_flow.prepare, SimpleNamespace(target="minor", dry_run=False))
            bump.assert_called_with(["bump", "minor"])
            draft.assert_called_with(["draft", "--version", target, "--replace"])
        with mock.patch.object(release, "main", return_value=0) as bump, mock.patch.object(changelog, "main", return_value=0) as draft:
            self.quiet(release_flow.prepare, SimpleNamespace(target="minor", dry_run=True))
            bump.assert_called_with(["bump", "minor", "--dry-run"])
            draft.assert_called_with(["draft", "--version", target, "--print"])
        with mock.patch.object(release, "main", return_value=0) as bump, mock.patch.object(changelog, "main", return_value=0) as draft:
            self.quiet(release_flow.prepare, SimpleNamespace(target="current", dry_run=True))
            bump.assert_not_called()
            draft.assert_called_with(["draft", "--version", str(current.version), "--print"])


if __name__ == "__main__":
    unittest.main()
