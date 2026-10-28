import contextlib
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import install_dev


class InstallGuardTests(unittest.TestCase):
    def test_signing_identity_is_unique_and_can_be_selected_explicitly(self):
        output = """\
  1) 1111111111111111111111111111111111111111 "Developer ID Application: Example (AAAAAAAAAA)"
  2) 2222222222222222222222222222222222222222 "Apple Development: Person One (BBBBBBBBBB)"
  3) 3333333333333333333333333333333333333333 "Apple Development: Person Two (CCCCCCCCCC)"
"""
        parsed = install_dev.signing_identities(output)
        self.assertEqual(len(parsed), 2)
        run = subprocess.CompletedProcess([], 0, stdout=output)
        with patch.object(install_dev.subprocess, "run", return_value=run):
            with self.assertRaisesRegex(SystemExit, "Set SOYEHT"):
                install_dev.development_signing_identity()
            self.assertEqual(
                install_dev.development_signing_identity(
                    "Apple Development: Person Two (CCCCCCCCCC)"
                ),
                (
                    "3333333333333333333333333333333333333333",
                    "Apple Development: Person Two (CCCCCCCCCC)",
                ),
            )

    def test_signed_binary_requires_stable_identifier_and_team(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            target = root / "target"
            source.write_bytes(b"binary")
            identity = ("1" * 40, "Apple Development: Person (ABCDEFGHIJ)")
            results = [
                subprocess.CompletedProcess([], 0),
                subprocess.CompletedProcess([], 0),
                subprocess.CompletedProcess(
                    [], 0,
                    stderr=(
                        f"Identifier={install_dev.BINARY_IDENTIFIERS['engine']}\n"
                        "Authority=Apple Development: Person (ABCDEFGHIJ)\n"
                        "TeamIdentifier=ZZZZZZZZZZ\n"
                    ),
                ),
                subprocess.CompletedProcess(
                    [], 0,
                    stdout=(
                        f'designated => identifier "{install_dev.BINARY_IDENTIFIERS["engine"]}" '
                        "and anchor apple generic\n"
                    ),
                    stderr="Executable=/private/tmp/soyeht-engine\n",
                ),
            ]
            with patch.object(install_dev.subprocess, "run", side_effect=results) as run:
                install_dev.copy_signed_binary(
                    source, target, install_dev.BINARY_IDENTIFIERS["engine"], identity
                )
            self.assertTrue(target.is_file())
            self.assertEqual(run.call_args_list[0].args[0][0], "codesign")
            self.assertIn(
                install_dev.BINARY_IDENTIFIERS["engine"], run.call_args_list[0].args[0]
            )

    def test_signed_binary_rejects_an_unstable_requirement_before_publish(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            target = root / "target"
            source.write_bytes(b"binary")
            identity = ("1" * 40, "Apple Development: Person (ABCDEFGHIJ)")
            results = [
                subprocess.CompletedProcess([], 0),
                subprocess.CompletedProcess([], 0),
                subprocess.CompletedProcess(
                    [], 0,
                    stderr=(
                        f"Identifier={install_dev.BINARY_IDENTIFIERS['engine']}\n"
                        "Authority=Apple Development: Person (ABCDEFGHIJ)\n"
                        "TeamIdentifier=ZZZZZZZZZZ\n"
                    ),
                ),
                subprocess.CompletedProcess(
                    [], 0,
                    stdout='designated => cdhash H"1234"\n',
                    stderr="Executable=/private/tmp/soyeht-engine\n",
                ),
            ]
            with patch.object(install_dev.subprocess, "run", side_effect=results):
                with self.assertRaisesRegex(SystemExit, "stable signing identity"):
                    install_dev.copy_signed_binary(
                        source, target, install_dev.BINARY_IDENTIFIERS["engine"], identity
                    )
            self.assertFalse(target.exists())

    def test_legacy_app_values_are_refused_before_external_work(self):
        legacy = (
            Path("/Applications/Soyeht.app/Contents"),
            Path.home() / "Library/Application Support/Soyeht",
            Path.home() / "Library/Application Support/Soyeht Dev/next",
            Path.home() / "Library/Application Support/Plain",
            Path.home() / "Library/Application Support/SOYEHT/next",
            Path("/APPLICATIONS/soyeht.app/next"),
        )
        for data in legacy:
            with self.subTest(data=data), patch.object(install_dev, "DATA", data), patch.object(
                install_dev.subprocess, "run"
            ) as run:
                with self.assertRaisesRegex(SystemExit, "Refusing data directory"):
                    install_dev.install_components(["engine"])
                run.assert_not_called()
        with patch.object(install_dev, "DEV_LAUNCHD_LABELS", {"engine": "com.soyeht.mac.engine", "ptyd": "x"}):
            with self.assertRaisesRegex(SystemExit, "Refusing service label"):
                install_dev.install_components(["engine"])

    def test_every_installed_value_of_the_profile_belongs_to_the_next_family(self):
        install_dev.refuse_legacy(install_dev.COMPONENTS)

    def test_unknown_service_cannot_reach_the_service_manager(self):
        with patch.object(install_dev.subprocess, "run") as run:
            with self.assertRaises(ValueError):
                install_dev.install_components(["other"])
            run.assert_not_called()

    def test_symlink_redirect_is_refused_before_external_work(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "actual").mkdir()
            (root / "next-redirect").symlink_to(root / "actual")
            with patch.object(install_dev, "DATA", root / "next-redirect"), patch.object(install_dev.subprocess, "run") as run:
                with self.assertRaises(SystemExit):
                    install_dev.install_components(["engine"])
                run.assert_not_called()

    @staticmethod
    @contextlib.contextmanager
    def checkout(*built):
        """A scratch checkout with the named programs built, and a Dev data folder beside it."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            for name in built:
                binary = root / "engine/target/debug" / name
                binary.parent.mkdir(parents=True, exist_ok=True)
                binary.write_bytes(name.encode())
            data = root / "new-next-dev"
            with patch.object(install_dev, "ROOT", root), patch.object(install_dev, "DATA", data):
                yield root, data

    def test_a_running_job_is_refused_before_copying_or_writing(self):
        with self.checkout("soyeht", "soyeht-engine") as (_, data):
            answer = [{"component": "engine", "running": True}]
            with patch.object(install_dev, "service_command", return_value=answer) as service:
                with self.assertRaisesRegex(SystemExit, "Stop the new Dev engine job"):
                    install_dev.install_components(["engine"])
            service.assert_called_once_with("status", "--component", "engine")
            self.assertFalse(data.exists())

    def test_the_service_manager_is_asked_through_the_shared_command(self):
        with self.checkout("soyeht") as (root, data):
            result = subprocess.CompletedProcess([], 0, stdout='{"services": []}')
            with patch.object(install_dev.subprocess, "run", return_value=result) as run:
                install_dev.service_command("install", "--component", "ptyd", "--programs", "x")
            command = run.call_args.args[0]
            self.assertEqual(command[0], str(root / "engine/target/debug/soyeht"))
            self.assertEqual(
                command[1:],
                ["--profile", "dev", "--data-dir", str(data), "service",
                 "install", "--component", "ptyd", "--programs", "x"],
            )

    def test_a_failing_service_command_stops_with_what_it_said(self):
        failure = subprocess.CalledProcessError(1, ["soyeht"], stderr='{"error":"no bus"}')
        with self.checkout("soyeht"):
            with patch.object(install_dev.subprocess, "run", side_effect=failure):
                with self.assertRaisesRegex(SystemExit, "service install failed: .*no bus"):
                    install_dev.service_command("install", "--component", "ptyd")

    def test_the_installer_writes_no_service_definition_itself(self):
        source = Path(install_dev.__file__).read_text()
        for word in ("plistlib", "launchctl", "LaunchAgents", "KeepAlive"):
            self.assertNotIn(word, source)

    def test_installing_requires_the_built_command_line_tool(self):
        with self.checkout(), patch.object(install_dev.subprocess, "run") as run:
            with self.assertRaisesRegex(SystemExit, "Build soyeht first"):
                install_dev.install_components(["engine"])
            run.assert_not_called()

    def test_engine_install_requires_the_sibling_mcp_binary(self):
        with self.checkout("soyeht", "soyeht-engine") as (_, data):
            answer = [{"component": "engine", "running": False}]
            with patch.object(install_dev, "service_command", return_value=answer):
                with self.assertRaisesRegex(SystemExit, "Build soyeht-mcp first"):
                    install_dev.install_components(["engine"])
            self.assertFalse(data.exists())


if __name__ == "__main__":
    unittest.main()
