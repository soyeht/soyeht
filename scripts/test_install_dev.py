import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import install_dev
from generated_profile import DEV_ENGINE_LAUNCHD_LABEL


class InstallGuardTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
