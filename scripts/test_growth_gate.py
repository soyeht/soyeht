import contextlib
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import growth_gate as growth
import hardening_gate
from gate_pr_text import statements

REASON = "the new protocol needs one test per frame kind"


class TestCodeTests(unittest.TestCase):
    def test_a_file_is_test_code_by_where_it_is_in_each_language(self):
        yes = (
            "engine/soyeht-engine/tests/runtime/a.rs", "engine/soyeht-avatar/src/tests/b.rs",
            "engine/soyeht-store/benches/c.rs", "apps/shared/Tests/SoyehtClientTests/A.swift",
            "apps/mac/PerformanceUITests/B.swift", "scripts/test_x.py", "contracts/test_y.py",
            "apps/android/app/src/test/kotlin/A.kt", "contracts/typescript/test/a.test.ts",
        )
        no = (
            "engine/soyeht-engine/src/lib.rs", "apps/shared/Sources/SoyehtClient/A.swift",
            "scripts/check_quality.py", "scripts/require_one_rust_test.py", "scripts/e2e.py",
            "apps/android/app/src/main/kotlin/A.kt", "contracts/vectors/frames.json", "docs/tests/a.md",
        )
        for path in yes:
            self.assertTrue(growth.in_test_location(path), path)
        for path in no:
            self.assertFalse(growth.in_test_location(path), path)

    def test_lines_count_in_test_files_cfg_test_items_and_test_module_files(self):
        sources = {
            "engine/a/tests/t.rs": "fn a() {}\n\n// note\nfn b() {}\n",
            "engine/a/src/lib.rs": (
                "mod helper;\n#[cfg(test)]\nmod checks;\nfn live() {}\n"
                "#[cfg(test)]\nmod tests {\n    // inside\n    #[test]\n    fn t() {}\n}\n"
                "struct S {\n    #[cfg(test)]\n    probe: u8,\n    kept: u8,\n}\n"
                "fn run() {\n    #[cfg(test)]\n    probe();\n    work();\n}\n"
            ),
            "engine/a/src/checks.rs": "fn x() {}\nfn y() {}\n",
            "engine/a/src/helper.rs": "fn h() {}\n",
            "scripts/test_k.py": "def test_k():\n    pass\n",
            "scripts/tool.py": "def tool():\n    pass\n",
            "engine/a/src/generated.rs": "// Generated from contracts/\n#[cfg(test)]\nmod t { fn x() {} }\n",
            "apps/shared/Tests/Generated.swift": "// Generated from contracts/\nlet x = 1\n",
        }
        self.assertEqual(growth.lines_of_test_code(sources), {
            "engine/a/tests/t.rs": 3, "engine/a/src/checks.rs": 2, "engine/a/src/lib.rs": 12,
            "scripts/test_k.py": 2,
        })


class AllowanceTests(unittest.TestCase):
    def check(self, base, head, body=""):
        errors = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(errors):
            allowances, _ = statements(body.splitlines(), growth.ALLOWANCE_LABEL)
            return growth.check_growth(base, head, allowances), errors.getvalue()

    def test_the_limit_is_net_so_deleting_elsewhere_pays_for_adding(self):
        self.assertEqual(self.check({"a": 0}, {"a": 400}), (False, ""))
        self.assertTrue(self.check({"a": 0}, {"a": 401})[0])
        self.assertFalse(self.check({"a": 300, "b": 500}, {"a": 700, "b": 400, "c": 100})[0])

    def test_more_needs_an_allowance_that_names_the_net_and_gives_a_reason(self):
        failed, message = self.check({}, {"a": 600})
        self.assertTrue(failed)
        for part in ("adds 600 net lines", "limit is 400", "consolidate", "delete tests elsewhere",
                     "Test growth allowance: <N> lines because <reason>", "from 600 to 660"):
            self.assertIn(part, message)
        for lines, passes in ((600, True), (660, True), (661, False), (599, False), (999999, False)):
            body = f"Test growth allowance: {lines} lines because {REASON}"
            self.assertEqual(not self.check({}, {"a": 600}, body)[0], passes, lines)
        for weak in ("Test growth allowance: 600 lines", "Test growth allowance: 600 lines because none",
                     "Test growth allowance: lots because it needs it badly", "600 lines because " + REASON):
            self.assertTrue(self.check({}, {"a": 600}, weak)[0], weak)
        self.assertIn("is not `<N> lines`", self.check({}, {"a": 600}, "Test growth allowance: lots because " + REASON)[1])
        self.assertIn("from 600 to 660", self.check({}, {"a": 600}, f"Test growth allowance: 9 lines because {REASON}")[1])


class MainTests(unittest.TestCase):
    """The whole check on a synthetic repository: the base is the commit, the head the work tree."""

    def run_main(self, lines, body=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "repo"
            (root / "engine/a/tests").mkdir(parents=True)
            (root / "engine/a/tests/t.rs").write_text("fn base() {}\n")
            for command in (["init", "-q"], ["add", "-A"], ["-c", "user.name=t", "-c", "user.email=t@x.y",
                                                             "commit", "-qm", "base"]):
                subprocess.run(["git", *command], cwd=root, check=True, capture_output=True)
            base = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
            (root / "engine/a/tests/t.rs").write_text("fn added() {}\n" * lines)
            arguments = ["growth_gate.py", "--base", base]
            if body is not None:
                event = Path(directory) / "event.json"
                event.write_text(json.dumps({"pull_request": {"body": body}}))
                arguments += ["--event-file", str(event)]
            errors = io.StringIO()
            with (mock.patch.object(growth, "ROOT", root), mock.patch.object(hardening_gate, "ROOT", root),
                  mock.patch.object(sys, "argv", arguments), contextlib.redirect_stdout(io.StringIO()),
                  contextlib.redirect_stderr(errors)):
                return growth.main(), errors.getvalue()

    def test_a_change_over_the_limit_fails_unless_its_body_carries_the_allowance(self):
        # The base file has one line, so a head of 401 lines is a net of 400.
        self.assertEqual(self.run_main(401)[0], 0)
        code, errors = self.run_main(402)
        self.assertEqual(code, 1)
        self.assertIn("adds 401 net lines of test code", errors)
        allowance = f"Test growth allowance: 401 lines because {REASON}"
        self.assertEqual(self.run_main(402, allowance)[0], 0)
        self.assertEqual(self.run_main(402, "Test growth allowance: 401 lines")[0], 1)


if __name__ == "__main__":
    unittest.main()
