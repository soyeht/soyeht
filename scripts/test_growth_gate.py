import contextlib
import io
import unittest

import growth_gate as growth
from gate_pr_text import statements

REASON = "the new protocol needs one test per frame kind"


def rust_module(count):
    return "".join(f"#[test]\nfn case_{n}() {{}}\n" for n in range(count))


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
            "scripts/check_quality.py", "apps/android/app/src/main/kotlin/A.kt",
            "contracts/vectors/frames.json", "contracts/vectors/test_cases.py", "docs/tests/a.md",
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
            ),
            "engine/a/src/checks.rs": "fn x() {}\nfn y() {}\n",
            "engine/a/src/helper.rs": "fn h() {}\n",
            "scripts/test_k.py": "def test_k():\n    pass\n",
            "scripts/tool.py": "def tool():\n    pass\n",
            "engine/a/src/generated.rs": "// Generated from contracts/\n#[cfg(test)]\nmod t { fn x() {} }\n",
            "apps/shared/Tests/Generated.swift": "// Generated from contracts/\nlet x = 1\n",
        }
        self.assertEqual(growth.lines_of_test_code(sources), {
            "engine/a/tests/t.rs": 3, "engine/a/src/checks.rs": 2, "engine/a/src/lib.rs": 6,
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

    def test_more_needs_an_allowance_that_covers_the_net_with_a_reason(self):
        line = f"Test growth allowance: 650 lines because {REASON}"
        failed, message = self.check({}, {"a": 600})
        self.assertTrue(failed)
        for part in ("adds 600 net lines", "limit is 400", "consolidate", "delete tests elsewhere",
                     "Test growth allowance: <N> lines because <reason>"):
            self.assertIn(part, message)
        self.assertFalse(self.check({}, {"a": 600}, line)[0])
        self.assertTrue(self.check({}, {"a": 700}, line)[0])
        for weak in ("Test growth allowance: 650 lines", "Test growth allowance: 650 lines because none",
                     "Test growth allowance: lots because it needs it badly", "650 lines because " + REASON):
            self.assertTrue(self.check({}, {"a": 600}, weak)[0], weak)


if __name__ == "__main__":
    unittest.main()
