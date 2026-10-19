import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

import check_editor_performance as gate


def run(key: float = 4.0) -> dict:
    return {
        "key_work_p95_ms": key,
        "key_event_samples": 240,
        "validated_key_samples": 240,
        "key_work_samples": 240,
        "file_line_count": 100_000,
        "visual_validation": "real_window_screenshot_and_gutter",
    }


def report(*runs: dict) -> dict:
    return {
        "configuration": "Release",
        "editor_tree_hash": "editor-hash",
        "measured_head": "old-engine-only-head",
        "unmeasured": gate.UNMEASURED,
        "runs": list(runs),
    }


class EditorPerformanceGateTests(unittest.TestCase):
    def test_release_proof_passes_even_when_only_head_changed(self):
        self.assertEqual(
            gate.report_failures(report(run(), run(), run()), "editor-hash"), [])

    def test_injected_slowdown_makes_gate_red(self):
        # This is the gate's production validator, not a one-off manual probe.
        result = gate.report_failures(
            report(run(key=8.2), run(key=8.7), run(key=7.0)), "editor-hash")
        self.assertTrue(any("key_work_p95_ms" in failure for failure in result))

    def test_scroll_and_page_load_must_be_marked_unmeasured(self):
        proof = report(run(), run(), run())
        del proof["unmeasured"]
        result = gate.report_failures(proof, "editor-hash")
        self.assertTrue(any("unmeasured" in failure for failure in result))

    def test_missing_visual_or_event_proof_is_red(self):
        bad = run()
        bad["visual_validation"] = "pending_ui_screenshot"
        bad["validated_key_samples"] = 239
        result = gate.report_failures(report(bad, run(), run()), "editor-hash")
        self.assertTrue(any("visible ink" in failure for failure in result))
        self.assertTrue(any("validated_key_samples" in failure for failure in result))

    def test_debug_or_editor_change_is_red(self):
        proof = report(run(), run(), run())
        proof["configuration"] = "Debug"
        result = gate.report_failures(proof, "changed-editor-hash")
        self.assertTrue(any("Release" in failure for failure in result))
        self.assertTrue(any("editor paths changed" in failure for failure in result))

    def test_editor_hash_ignores_unrelated_engine_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q"], cwd=root, check=True)
            editor = root / "apps/mac/Sources/EditorNativeViewport.swift"
            unrelated = root / "engine/soyeht-engine/src/main.rs"
            editor.parent.mkdir(parents=True)
            unrelated.parent.mkdir(parents=True)
            editor.write_text("editor")
            unrelated.write_text("engine")
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            initial = gate.editor_tree_hash(root)
            unrelated.write_text("different engine")
            self.assertEqual(gate.editor_tree_hash(root), initial)
            editor.write_text("different editor")
            self.assertNotEqual(gate.editor_tree_hash(root), initial)

    def rust_check(self, open_ms: float):
        with tempfile.TemporaryDirectory() as directory:
            rust_report = Path(directory) / "rust.json"

            def fake_cargo(_command, **_kwargs):
                rust_report.write_text(json.dumps({"open_100k_ms": open_ms, "search_1gb_ms": 10.0}))

            with (
                mock.patch.object(gate, "CACHE", Path(directory)),
                mock.patch.object(gate, "RUST_REPORT", rust_report),
                mock.patch.object(gate.subprocess, "run", side_effect=fake_cargo) as cargo,
                mock.patch.object(gate.os, "getloadavg", return_value=(61.5, 40.0, 30.0)),
                mock.patch.object(gate.os, "cpu_count", return_value=24),
            ):
                try:
                    return gate.check_rust(), cargo
                except RuntimeError as error:
                    return error, cargo

    def test_the_rust_half_is_its_own_step_and_names_the_load_when_it_fails(self):
        rust, cargo = self.rust_check(92.0)
        self.assertEqual(rust["open_100k_ms"], 92.0)
        self.assertIn("editor_performance", cargo.call_args.args[0])
        error, _ = self.rust_check(243.0)
        self.assertIsInstance(error, RuntimeError)
        self.assertIn("open_100k_ms=243.0 ms >= 150.0 ms", str(error))
        self.assertIn("load average 61.5 on 24 cpus", str(error))

    def test_the_vm_check_no_longer_runs_the_rust_benchmark(self):
        with tempfile.TemporaryDirectory() as directory:
            vm_report = Path(directory) / "vm.json"
            vm_report.write_text(json.dumps(report(run(), run(), run())))
            with (
                mock.patch.object(gate, "VM_REPORT", vm_report),
                mock.patch.object(gate, "REPORT", Path(directory) / "report.json"),
                mock.patch.object(gate, "editor_tree_hash", return_value="editor-hash"),
                mock.patch.object(gate, "head", return_value="head"),
                mock.patch.object(gate.subprocess, "run") as cargo,
            ):
                self.assertNotIn("rust", gate.check())
            cargo.assert_not_called()

    def test_vm_command_uses_test_runner_environment_and_redacts_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            seen = []

            def fake_xcodebuild(_command, **kwargs):
                environment = kwargs["env"]
                seen.append(environment.copy())
                Path(environment["TEST_RUNNER_SOYEHT_EDITOR_PERFORMANCE_REPORT"]).write_text(
                    json.dumps(run()))
                return SimpleNamespace(
                    returncode=0,
                    stdout="Apple Development: Example Person (EXAMPLE123)\nteam=EXAMPLE123\n",
                )

            with (
                mock.patch.object(gate, "CACHE", cache),
                mock.patch.object(gate, "VM_REPORT", cache / "editor-performance-vm.json"),
                mock.patch.object(gate, "editor_tree_hash", return_value="editor-hash"),
                mock.patch.object(gate, "head", return_value="measured-head"),
                mock.patch.object(gate.subprocess, "run", side_effect=fake_xcodebuild),
                mock.patch.dict(
                    "os.environ",
                    {"SOYEHT_EDITOR_METER_VM": "1", "SOYEHT_DEVELOPMENT_TEAM": "EXAMPLE123"},
                    clear=True,
                ),
            ):
                gate.run_vm_measurement()
            self.assertEqual(len(seen), 3)
            self.assertTrue(all(
                "TEST_RUNNER_SOYEHT_EDITOR_METER_APP_BUNDLE_ID" in entry
                and "SOYEHT_EDITOR_PERFORMANCE_REPORT" not in entry
                for entry in seen))
            log = (cache / "editor-performance-run-1.log").read_text()
            self.assertNotIn("Example Person", log)
            self.assertNotIn("EXAMPLE123", log)


if __name__ == "__main__":
    unittest.main()
