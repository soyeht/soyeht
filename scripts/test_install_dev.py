import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import install_dev
from generated_profile import DEV_ENGINE_LAUNCHD_LABEL


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

    def test_service_path_includes_system_and_user_tool_commands(self):
        environment = install_dev.service_environment()
        paths = environment["PATH"].split(":")
        self.assertEqual(paths[:-2], list(install_dev.SYSTEM_PATHS))
        self.assertEqual(
            paths[-2:],
            [str(Path.home() / ".local/bin"), str(Path.home() / ".cargo/bin")],
        )

    def test_unknown_service_or_domain_cannot_reach_launchctl(self):
        with patch.object(install_dev.subprocess, "run") as run:
            for components, domain in ((["other"], "gui"), (["engine"], "system")):
                with self.assertRaises(ValueError):
                    install_dev.install_components(components, domain)
            run.assert_not_called()

    def test_symlink_redirect_is_refused_before_external_work(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / "actual").mkdir()
            (root / "redirect").symlink_to(root / "actual")
            with patch.object(install_dev, "DATA", root / "redirect"), patch.object(install_dev.subprocess, "run") as run:
                with self.assertRaises(SystemExit):
                    install_dev.install_components(["engine"])
                run.assert_not_called()

    def test_loaded_job_is_refused_before_copying_or_writing(self):
        with tempfile.TemporaryDirectory() as temporary:
            data = Path(temporary).resolve() / "new-dev"
            with patch.object(install_dev, "DATA", data), patch.object(install_dev.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as run:
                with self.assertRaises(SystemExit):
                    install_dev.install_components(["engine"])
                self.assertFalse(data.exists())
                self.assertEqual(len(run.call_args_list), 2)
                self.assertTrue(
                    run.call_args_list[1].args[0][-1].endswith(
                        "/" + DEV_ENGINE_LAUNCHD_LABEL
                    )
                )

    def test_engine_install_requires_the_sibling_mcp_binary(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            data = root / "new-dev"
            binary = root / "engine/target/debug/soyeht-engine"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"engine")
            results = [
                subprocess.CompletedProcess([], 0),
                subprocess.CompletedProcess([], 1),
            ]
            with patch.object(install_dev, "ROOT", root), patch.object(
                install_dev, "DATA", data
            ), patch.object(install_dev.subprocess, "run", side_effect=results):
                with self.assertRaisesRegex(SystemExit, "Build soyeht-mcp first"):
                    install_dev.install_components(["engine"])
                self.assertFalse(data.exists())


if __name__ == "__main__":
    unittest.main()
