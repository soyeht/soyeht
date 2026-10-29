import contextlib
import io
import json
import os
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import check_editor_performance
import gate_worktree
import post_merge_editor
from test_post_merge_fixtures import FAKE_EDITOR_GATE, SyntheticRepositoryCase


class EditorMeasurementTests(SyntheticRepositoryCase):
    """The VM1 report is copied into the worktree; a missing or stale one is not a red main."""

    FILES = {"scripts/check_all.sh": FAKE_EDITOR_GATE, "a": "1"}

    def setUp(self):
        super().setUp()
        self.trace = self.root.parent / "trace.txt"
        self.meters = self.root.parent / "meters"
        self.meters.mkdir()
        self.editor_hash = check_editor_performance.editor_tree_hash(self.root)
        environment = mock.patch.dict(os.environ, {"FAKE_TRACE": str(self.trace)})
        environment.start()
        self.addCleanup(environment.stop)

    def meter(self, content="ok", name="report.json", **fields):
        path = self.meters / name
        path.write_text(json.dumps({"editor_tree_hash": self.editor_hash, "state": content, **fields}))
        return path

    def runs(self):
        return self.trace.read_text().splitlines() if self.trace.exists() else []

    def last_green(self):
        return json.loads((self.health / "last_green.json").read_text())["sha"]

    def green_start(self):
        self.assertEqual(self.run_main(self.shas[0], "--editor-meter", str(self.meter())), 0)
        self.trace.unlink()

    def test_the_report_is_copied_into_the_runs_worktree(self):
        self.assertEqual(self.run_main(self.shas[0], "--editor-meter", str(self.meter()), "--keep"), 0)
        self.assertEqual(self.record(self.shas[0])["editor_meter"], "copied")
        kept = next(self.scratch.glob(f"{gate_worktree.WORKTREE_PREFIX}*/tree"))
        self.assertEqual(json.loads((kept / ".cache/editor-performance-vm.json").read_text())["state"], "ok")

    def test_the_environment_names_the_report_too(self):
        with mock.patch.dict(os.environ, {post_merge_editor.EDITOR_METER_ENV: str(self.meter())}):
            self.assertEqual(self.run_main(self.shas[0]), 0)
        self.assertEqual(self.record(self.shas[0])["editor_meter"], "copied")

    def test_a_missing_measurement_is_recorded_and_blames_nobody(self):
        self.green_start()
        self.add("two", {"b": "2"})
        head = self.add("three", {"c": "3"})
        self.assertEqual(self.run_main(head), post_merge_editor.NEEDS_MEASUREMENT_EXIT)
        record = self.record(head)
        self.assertEqual(record["result"], "needs-measurement")
        self.assertEqual(record["failed_step"], "editor-performance:check")
        self.assertEqual(record["editor_tree_hash"], self.editor_hash)
        self.assertEqual(record["editor_meter"], "not-given")
        self.assertIn(self.editor_hash, record["note"])
        self.assertNotIn("culprit", record)
        self.assertNotIn("candidates", record)
        self.assertEqual([step["id"] for step in record["steps"]], ["unit", "editor-performance:check", "ts"])
        self.assertEqual(len(self.runs()), 2, "the gate, then the steps the editor check hid; no search")
        self.assertEqual(self.last_green(), head, "every step but the measurement passed: a good main")
        self.assertEqual(json.loads((self.health / "latest.json").read_text())["sha"], head)

    def test_a_flaky_step_before_the_editor_check_still_leaves_the_measurement_to_vm1(self):
        self.green_start()
        head = self.add("two", {"b": "2"})
        once = self.root.parent / "once"
        once.write_text("")
        with mock.patch.dict(os.environ, {"FAKE_ONCE": str(once)}):
            self.assertEqual(self.run_main(head), post_merge_editor.NEEDS_MEASUREMENT_EXIT)
        record = self.record(head)
        self.assertEqual(record["result"], "needs-measurement")
        self.assertEqual([flake["step"] for flake in record["flaky_steps"]], ["unit"])
        self.assertEqual([step["id"] for step in record["steps"]], ["unit", "editor-performance:check", "ts"])
        self.assertNotIn("culprit", record)
        ledger = json.loads((self.health / "flakes.json").read_text())
        self.assertEqual([(flake["step"], flake["sha"]) for flake in ledger["flakes"]], [("unit", head)])

    def test_a_main_that_only_lacks_the_measurement_starts_the_culprit_search(self):
        first = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(first), post_merge_editor.NEEDS_MEASUREMENT_EXIT)
        self.assertEqual(self.last_green(), first)
        self.add("three", {"c": "3"})
        culprit = self.add("breaks the last step", {"broken": "x"})
        head = self.add("five", {"d": "4"})
        with mock.patch.dict(os.environ, {"FAKE_BREAK": "ts"}):
            self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertEqual(record["result"], "fail")
        self.assertEqual(record["culprit"]["sha"], culprit)
        self.assertNotIn("no known green main", record.get("note", ""))
        self.assertEqual(self.last_green(), first, "a red main does not move it")

    def test_a_report_that_is_not_there_is_the_same_as_none(self):
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(head, "--editor-meter", str(self.meters / "gone.json")),
                         post_merge_editor.NEEDS_MEASUREMENT_EXIT)
        self.assertEqual(self.record(head)["editor_meter"], "missing")

    def test_a_report_of_other_editor_files_is_a_missing_measurement(self):
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(head, "--editor-meter", str(self.meter("stale"))),
                         post_merge_editor.NEEDS_MEASUREMENT_EXIT)
        self.assertEqual(self.record(head)["result"], "needs-measurement")

    def test_a_latency_over_the_limit_stays_red(self):
        self.green_start()
        head = self.add("slows the editor", {"b": "2"})
        self.assertEqual(self.run_main(head, "--editor-meter", str(self.meter("slow"))), 1)
        record = self.record(head)
        self.assertEqual(record["result"], "fail")
        self.assertEqual(record["failed_step"], "editor-performance:check")
        self.assertEqual(record["culprit"]["sha"], head)
        self.assertNotIn("editor_tree_hash", record)

    def test_a_step_after_the_editor_check_still_decides_the_run(self):
        self.green_start()
        head = self.add("breaks the last step", {"broken": "x"})
        with mock.patch.dict(os.environ, {"FAKE_BREAK": "ts"}):
            self.assertEqual(self.run_main(head), 1)
        record = self.record(head)
        self.assertEqual((record["result"], record["failed_step"]), ("fail", "ts"))
        self.assertEqual(record["culprit"]["sha"], head)
        self.assertEqual(self.last_green(), self.shas[0], "the missing measurement does not hide a failed step")

    def test_a_recorded_run_waits_for_a_report_of_its_hash(self):
        head = self.add("two", {"b": "2"})
        self.assertEqual(self.run_main(head), post_merge_editor.NEEDS_MEASUREMENT_EXIT)
        runs = len(self.runs())
        self.assertEqual(self.run_main(head), post_merge_editor.NEEDS_MEASUREMENT_EXIT)  # nothing new to say
        other = self.meter(name="other.json", editor_tree_hash="0" * 64)
        self.assertEqual(self.run_main(head, "--editor-meter", str(other)), post_merge_editor.NEEDS_MEASUREMENT_EXIT)
        self.assertEqual(len(self.runs()), runs, "no second run until the report measures this hash")
        self.assertEqual(self.run_main(head, "--editor-meter", str(self.meter())), 0)
        self.assertEqual(self.record(head)["result"], "pass")
        self.assertGreater(len(self.runs()), runs)

    def test_a_stale_report_on_an_older_commit_does_not_blame_it(self):
        """The probes run older commits with the newest report: stale says nothing about latency."""
        self.add("editor v1", {"editor_version": "v1"})
        self.assertEqual(self.run_main(self.shas[-1], "--editor-meter", str(self.meter("ok v1"))), 0)
        self.add("unrelated", {"b": "2"})
        self.add("editor v2", {"editor_version": "v2"})
        self.add("unrelated too", {"c": "3"})
        head = self.add("unrelated again", {"d": "4"})
        self.assertEqual(self.run_main(head, "--editor-meter", str(self.meter("slow v2"))), 1)
        record = self.record(head)
        self.assertEqual((record["result"], record["failed_step"]), ("fail", "editor-performance:check"))
        self.assertNotIn("culprit", record, "a commit that fails as stale is skipped, not blamed")
        self.assertEqual([c["subject"] for c in record["candidates"]], ["unrelated", "editor v2"])


class EditorMessageContractTests(unittest.TestCase):
    """The log lines that make a run `needs-measurement` are the ones check_editor_performance prints."""

    def printed(self, vm_report: Path) -> str:
        output = io.StringIO()
        with mock.patch.object(check_editor_performance, "VM_REPORT", vm_report), \
                mock.patch.object(sys, "argv", ["check_editor_performance.py"]), \
                contextlib.redirect_stderr(output):
            self.assertEqual(check_editor_performance.main(), 1)
        return output.getvalue()

    def log_of(self, text: str) -> Path:
        log = Path(tempfile.mkdtemp()) / "gate.log"
        self.addCleanup(shutil.rmtree, log.parent, ignore_errors=True)
        log.write_text(text)
        return log

    def test_a_missing_report_is_recognised(self):
        message = self.printed(Path(tempfile.gettempdir()) / "no-such-report" / "editor.json")
        self.assertTrue(message.startswith(post_merge_editor.MISSING_REPORT), message)
        self.assertTrue(post_merge_editor.lacks_measurement(self.log_of(message)))

    def test_a_report_of_other_editor_files_is_recognised(self):
        directory = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, directory, ignore_errors=True)
        report = directory / "editor.json"
        report.write_text(json.dumps({"configuration": "Release", "editor_tree_hash": "of another version"}))
        message = self.printed(report)
        self.assertIn(post_merge_editor.STALE_REPORT, message)
        self.assertTrue(post_merge_editor.lacks_measurement(self.log_of(message)))
        self.assertIn(post_merge_editor.STALE_REPORT, check_editor_performance.report_failures({}, "hash"))

    def test_a_latency_over_the_limit_is_not_a_missing_measurement(self):
        run = {"key_event_samples": 240, "validated_key_samples": 240, "key_work_samples": 240,
               "visual_validation": "real_window_screenshot_and_gutter", "file_line_count": 100_000,
               "key_work_p95_ms": 9.0}
        report = {"configuration": "Release", "editor_tree_hash": "h", "unmeasured": check_editor_performance.UNMEASURED,
                  "runs": [run] * check_editor_performance.RUN_COUNT}
        failures = check_editor_performance.report_failures(report, "h")
        self.assertEqual(len(failures), 1)
        self.assertFalse(post_merge_editor.lacks_measurement(self.log_of("editor-performance: " + "; ".join(failures))))


if __name__ == "__main__":
    unittest.main()
