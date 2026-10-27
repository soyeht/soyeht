"""The agent precheck takes its steps from check_all.sh and reports every failure."""

from __future__ import annotations

import contextlib
import io
import subprocess
import unittest
from pathlib import Path
from unittest import mock

import agent_precheck
from agent_precheck import (
    CHECK_ALL, Result, affected_by, format_roots, import_closure, main, report, select_steps, unit_tests,
)

BASE = "a" * 40
STEPS = [
    ("gate_scripts", "post-merge:unit"),
    ("always", "forbidden-terms:unit"),
    ("always", "forbidden-terms:scan"),
    ("always", "layers:unit"),
    ("always", "layers:check"),
    ("always", "quality:check"),
    ("always", "contracts:generate-check"),
    ("always", "contracts:unit"),
    ("engine", "engine:fmt"),
    ("swift", "swift:format-lint"),
]
UNITS = {
    "post-merge:unit": "test_post_merge.py",
    "forbidden-terms:unit": "test_forbidden_terms.py",
    "layers:unit": "test_check_layers.py",
}
TEXTS = {
    "test_post_merge.py": "import post_merge\n",
    "post_merge.py": "from gate_base import main_checkout\nimport json\n",
    "gate_base.py": "import re\n",
    "test_forbidden_terms.py": "from forbidden_terms import main\n",
    "forbidden_terms.py": "import os\n",
    "test_check_layers.py": "from check_layers import LAYERS\nimport gate_plan\n",
    "check_layers.py": "import tomllib\n",
    "gate_plan.py": "import gate_base\n",
}


def read(path: Path) -> str:
    if path.name not in TEXTS:
        raise FileNotFoundError(path)
    return TEXTS[path.name]


class SelectionTests(unittest.TestCase):
    def test_every_always_step_of_the_gate_is_in_the_precheck_or_named_as_left_out(self):
        text = CHECK_ALL.read_text()
        listed = [(line.split()[1], line.split()[3]) for line in text.splitlines()
                  if line.startswith(("step always ", "step gate_scripts "))]
        units = unit_tests(text)
        self.assertIn("forbidden-terms:unit", units)
        everything = ["scripts/" + path.name for path in (agent_precheck.ROOT / "scripts").glob("*.py")]
        cheap, contracts, quality = select_steps(listed, units, everything, lambda path: path.read_text())
        chosen = set(cheap) | set(contracts)
        left_out = set(agent_precheck.OWN_STEPS) | {agent_precheck.QUALITY_STEP}
        self.assertTrue(quality)
        self.assertIn("gate_scripts", {area for area, _ in listed})
        for _, step_id in listed:
            with self.subTest(step=step_id):
                self.assertEqual(step_id in chosen, step_id not in left_out)

    def test_each_unit_step_names_a_test_file_that_exists(self):
        for step_id, test in unit_tests(CHECK_ALL.read_text()).items():
            with self.subTest(step=step_id):
                self.assertTrue((agent_precheck.ROOT / "scripts" / test).is_file())

    def test_the_scan_and_quality_are_not_cheap_steps_and_quick_units_run_with_gate_tooling(self):
        cheap, contracts, quality = select_steps(STEPS, UNITS, ["scripts/forbidden_terms.py"], read)
        self.assertEqual(cheap, ["forbidden-terms:unit", "layers:unit", "layers:check"])
        self.assertEqual(contracts, ["contracts:generate-check", "contracts:unit"])
        self.assertTrue(quality)

    def test_a_workflow_or_hook_change_runs_the_quick_unit_steps(self):
        for path in (".github/workflows/gate.yml", ".githooks/pre-push", "scripts/check_all.sh"):
            with self.subTest(path=path):
                cheap, _, _ = select_steps(STEPS, UNITS, [path], read)
                self.assertIn("layers:unit", cheap)
                self.assertNotIn("post-merge:unit", cheap)

    def test_a_slow_unit_step_runs_for_a_module_it_imports_at_any_depth(self):
        for changed in (["scripts/test_post_merge.py"], ["scripts/post_merge.py"], ["scripts/gate_base.py"]):
            with self.subTest(changed=changed):
                cheap, _, _ = select_steps(STEPS, UNITS, changed, read)
                self.assertIn("post-merge:unit", cheap)
        cheap, _, _ = select_steps(STEPS, UNITS, ["scripts/gate_plan.py"], read)
        self.assertNotIn("post-merge:unit", cheap)

    def test_the_import_closure_is_transitive_and_leaves_libraries_out(self):
        self.assertEqual(import_closure("test_post_merge.py", read),
                         {"test_post_merge.py", "post_merge.py", "gate_base.py"})

    def test_a_file_the_test_names_counts_and_an_unreadable_test_cannot_be_ruled_out(self):
        texts = {"test_gate_list.py": "ROOT / 'scripts/check_all.sh'"}
        self.assertTrue(affected_by("test_gate_list.py", ["scripts/check_all.sh"],
                                    lambda path: texts[path.name]))
        self.assertTrue(affected_by("test_missing.py", [], mock.Mock(side_effect=OSError)))

    def test_nothing_runs_for_a_change_outside_the_scripts(self):
        cheap, _, _ = select_steps(STEPS, UNITS, ["engine/soyeht-web/src/lib.rs"], read)
        self.assertEqual(cheap, ["layers:check"])

    def test_the_slow_units_are_steps_of_the_gate(self):
        units = unit_tests(CHECK_ALL.read_text())
        for step_id in agent_precheck.SLOW_UNITS:
            self.assertIn(step_id, units)


class RunTests(unittest.TestCase):
    def run_main(self, results, *arguments, base=BASE, fetch=None, listed=None):
        outcomes = iter(results)
        output = io.StringIO()
        with (mock.patch.object(agent_precheck, "fetch_main", return_value=fetch),
              mock.patch.object(agent_precheck, "resolve_base", return_value=base),
              mock.patch.object(agent_precheck, "changed_files", return_value=["a.swift"]),
              mock.patch.object(agent_precheck, "listed_steps", return_value=listed or STEPS),
              mock.patch.object(agent_precheck, "gate_step",
                                side_effect=lambda step_id: Result(step_id, next(outcomes), 0.1, "boom\n")),
              mock.patch.object(agent_precheck, "message_checks", return_value=[Result("msgs", True, 0.1)]),
              mock.patch.object(agent_precheck, "swift_format", return_value=[Result("swift", True, 0.1)]),
              contextlib.redirect_stdout(output)):
            code = main(list(arguments))
        return code, output.getvalue()

    def test_every_failure_is_reported_not_only_the_first(self):
        # "a.swift" is not gate tooling, so no unit step runs
        # layers:check, contracts:generate-check, contracts:unit, quality:check
        code, output = self.run_main([False, True, False, True])
        self.assertEqual(code, 1)
        self.assertIn("FAILED: layers:check, contracts:unit", output)
        self.assertIn("    boom", output)

    def test_all_green_passes_and_quick_leaves_quality_out(self):
        code, output = self.run_main([True, True, True, True])
        self.assertEqual((code, "quality:check" in output), (0, True))
        code, output = self.run_main([True, True, True], "--quick")
        self.assertEqual((code, "quality:check" in output), (0, False))
        self.assertIn("check_quality skipped", output)

    def test_a_precheck_that_cannot_resolve_its_base_exits_two(self):
        errors = io.StringIO()
        with (mock.patch.object(agent_precheck, "fetch_main", side_effect=RuntimeError("no origin")),
              contextlib.redirect_stderr(errors)):
            self.assertEqual(main([]), 2)
        self.assertIn("no origin", errors.getvalue())

    def test_a_list_with_no_always_step_is_a_failure_to_run_not_a_pass(self):
        errors = io.StringIO()
        with contextlib.redirect_stderr(errors):
            code, _ = self.run_main([], listed=[("engine", "engine:fmt")])
        self.assertEqual(code, 2)
        self.assertIn("no `always` step parsed", errors.getvalue())

    def test_an_unexpected_exception_exits_two(self):
        errors = io.StringIO()
        with (mock.patch.object(agent_precheck, "fetch_main", side_effect=FileNotFoundError("git")),
              contextlib.redirect_stderr(errors)):
            self.assertEqual(main([]), 2)
        self.assertIn("unexpected FileNotFoundError", errors.getvalue())

    def test_a_failure_prints_the_command_that_reruns_it(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            report(Result("layers:check", False, 1.0, "bad\n", "bash scripts/check_all.sh --only layers:check"))
        self.assertIn("    rerun: bash scripts/check_all.sh --only layers:check", output.getvalue())

    def test_a_failed_fetch_with_an_older_origin_is_a_warning(self):
        code, output = self.run_main([True, True, True, True], fetch="could not fetch origin/main")
        self.assertEqual(code, 0)
        self.assertIn("warning: could not fetch", output)


class GitTests(unittest.TestCase):
    def test_changes_are_commits_work_and_untracked_files_without_duplicates(self):
        outputs = {"diff": "b.py\na.py\n", "ls-files": "a.py\nnew.swift\n"}
        with mock.patch.object(agent_precheck, "git", side_effect=lambda *args: outputs[args[0]]):
            self.assertEqual(agent_precheck.changed_files(BASE), ["a.py", "b.py", "new.swift"])

    def test_the_lint_roots_are_the_gates_and_exclude_packages_and_generated_swift(self):
        roots = format_roots(CHECK_ALL.read_text())
        self.assertEqual(roots, ["apps/shared/Sources", "apps/shared/Tests", "apps/dictation/Sources",
                                 "apps/dictation/Tests", "apps/mac/Sources", "apps/ios/Sources"])

    def test_swift_format_runs_on_existing_changed_swift_files_of_the_gates_roots_only(self):
        roots = ["apps/shared/Sources", "apps/mac/Sources"]
        linted = "apps/shared/Sources/SoyehtClient/AgentPaneTicker.swift"
        self.assertTrue((agent_precheck.ROOT / linted).is_file())
        not_linted = ["apps/shared/Package.swift", "contracts/swift/Sources/SoyehtProtocol/Generated.swift",
                      "apps/ios/Sources/Gone.swift", "x.py", "apps/shared/SourcesExtra/A.swift"]
        with (mock.patch.object(agent_precheck, "timed", return_value=Result("swift-format", True, 0)) as timed,
              mock.patch.object(agent_precheck.shutil, "which", return_value="/usr/bin/swift")):
            agent_precheck.swift_format(not_linted, roots)
            timed.assert_not_called()
            agent_precheck.swift_format([*not_linted, linted], roots)
            self.assertEqual(timed.call_args.args[1][-1], linted)
        with mock.patch.object(agent_precheck.shutil, "which", return_value=None):
            result = agent_precheck.swift_format([linted], roots)
        self.assertTrue(result[0].ok)
        self.assertIn("skipped", result[0].output)

    def test_a_fetch_that_times_out_is_a_warning_when_an_older_origin_exists(self):
        calls = [subprocess.TimeoutExpired("git", 60), mock.Mock(returncode=0)]
        with mock.patch.object(agent_precheck, "run", side_effect=calls):
            self.assertIn("could not fetch", agent_precheck.fetch_main())
        calls = [subprocess.TimeoutExpired("git", 60), mock.Mock(returncode=1)]
        with mock.patch.object(agent_precheck, "run", side_effect=calls), self.assertRaises(RuntimeError):
            agent_precheck.fetch_main()

    def test_the_vocabulary_scan_joins_the_message_scan_only_when_it_is_supplied(self):
        with mock.patch.object(agent_precheck, "timed", return_value=Result("x", True, 0)) as timed:
            with mock.patch.dict(agent_precheck.os.environ, {"FORBIDDEN_TERMS": ""}):
                self.assertEqual(len(agent_precheck.message_checks(BASE, [Path("pr.md")])), 1)
            self.assertIn("--messages-only", timed.call_args.args[1])
            self.assertIn("pr.md", timed.call_args.args[1])
            with mock.patch.dict(agent_precheck.os.environ, {"FORBIDDEN_TERMS": "term"}):
                self.assertEqual(len(agent_precheck.message_checks(BASE, [])), 2)


if __name__ == "__main__":
    unittest.main()
