import contextlib
import io
import re
import unittest
from unittest import mock

import gate_vectors
from gate_vectors import GATE, HELPERS, ROOT, UNCLASSIFIED, reader_group, readers, vectors_read

NAMES = ["usage", "usage-presentation", "update", "markdown"]


class ReaderRuleTests(unittest.TestCase):
    """What counts as a read of a vector file, and where a reader's tests run."""

    def read(self, text: str, path: str = "") -> set[str]:
        return vectors_read(text, NAMES, path)

    def test_a_path_and_a_loader_call_are_reads(self):
        self.assertEqual(self.read('include_str!("../../contracts/vectors/usage.json")'), {"usage"})
        self.assertEqual(self.read('try ContractVector.load("update")'), {"update"})
        self.assertEqual(self.read('crate::testing::vector("usage-presentation")'), {"usage-presentation"})
        self.assertEqual(self.read('testing::cases("markdown", "runs")'), {"markdown"})
        self.assertEqual(self.read('vectors_dir().join("markdown.json")'), {"markdown"})

    def test_a_name_that_only_starts_like_another_is_not_it(self):
        self.assertEqual(self.read('vector("usage-presentation")'), {"usage-presentation"})
        self.assertEqual(self.read("contracts/vectors/usage-presentation.json"), {"usage-presentation"})

    def test_a_comment_or_a_key_of_the_same_name_is_not_a_read(self):
        self.assertEqual(self.read("// the cases of contracts/vectors/usage.json\n/// vector(\"update\")"), set())
        self.assertEqual(self.read('let key = "markdown"; vector_count()'), set())
        self.assertEqual(self.read('join("markdown.json")'), set())

    def test_a_hash_opens_a_comment_only_where_the_language_has_one(self):
        swift = '  #expect(try ContractVector.load("update").count > 0)'
        self.assertEqual(self.read(swift, "apps/shared/Tests/A.swift"), {"update"})
        self.assertEqual(self.read('# vector("update")', "contracts/a.py"), set())
        self.assertEqual(self.read('#[test]\nfn a() { crate::testing::vector("update"); }', "a.rs"), {"update"})

    def test_a_reader_runs_where_its_directory_says(self):
        for path, group in (
            ("apps/shared/Tests/SoyehtClientTests/A.swift", "apps/shared"),
            ("apps/desktop/crates/soyeht-client/src/a.rs", "apps/desktop"),
            ("contracts/typescript/test/a.test.ts", "contracts/typescript"),
            ("contracts/swift/Sources/ProtocolVectors/main.swift", "contracts/swift"),
            ("engine/soyeht-avatar/src/tests/vectors/mod.rs", "engine/soyeht-avatar"),
            ("engine/soyeht-protocol/tests/vectors.rs", "engine/soyeht-protocol/tests"),
            ("engine/soyeht-engine/tests/runtime/a.rs", "engine/soyeht-engine/tests"),
            ("contracts/update.py", GATE),
            ("scripts/check_swift.sh", GATE),
            ("apps/mac/Sources/A.swift", UNCLASSIFIED),
        ):
            with self.subTest(path=path):
                self.assertEqual(reader_group(path), group)

    def test_only_a_json_file_directly_under_the_vectors_is_a_vector(self):
        self.assertEqual(gate_vectors.vector_name("contracts/vectors/voice-turns.json"), "voice-turns")
        for path in ("contracts/vectors/sub/a.json", "contracts/vectors/a.md", "contracts/a.json"):
            with self.subTest(path=path):
                self.assertIsNone(gate_vectors.vector_name(path))


class ScanTests(unittest.TestCase):
    """The scan finds every reader, or the test says which source it cannot read."""

    def test_every_vector_file_has_a_reader_and_every_reader_a_group(self):
        found = readers()
        self.assertEqual(sorted(found), gate_vectors.vector_names())
        for name, groups in found.items():
            with self.subTest(vector=name):
                self.assertTrue(set(groups) - {GATE}, "no source runs by the gate reads it")
                self.assertNotIn(UNCLASSIFIED, groups, "a reader in no directory of READER_AREAS or the engine")

    def test_a_source_that_holds_the_vectors_directory_without_naming_a_vector_is_a_known_helper(self):
        # `vector(name)` reads a vector by a name the scan cannot see: it is found by the function that
        # takes it. A new helper goes in HELPERS, with the names of its functions.
        names = gate_vectors.vector_names()
        for path, text in gate_vectors.source_files():
            code = gate_vectors.code_lines(text, path)
            if gate_vectors.VECTORS.rstrip("/") in code and not vectors_read(text, names, path) and reader_group(path) != GATE:
                with self.subTest(file=path):
                    self.assertIn(path, HELPERS)
        for path, functions in HELPERS.items():
            for function in functions:
                with self.subTest(helper=path, function=function):
                    self.assertRegex((ROOT / path).read_text(), rf"(?:fn|func|def)\s+{function}\b")

    def test_no_program_embeds_a_vector(self):
        # A vector file selects the tests of a reader and builds no program: that holds while the engine and
        # the desktop read vectors only in test code (a `#[cfg(test)]` item, or a `tests/` directory or `tests.rs` file).
        for path, text in gate_vectors.source_files():
            if path.startswith(("engine/", "apps/desktop/")) and "/src/" in path and not re.search(r"/tests/|tests?\.rs$", path):
                production = text.split("#[cfg(test)]")[0]
                with self.subTest(file=path):
                    self.assertNotRegex(gate_vectors.code_lines(production),
                                        r"include_(?:str|bytes)!\([^)]*contracts/vectors")


class StepTests(unittest.TestCase):
    """`swift:test-shared-changed` and `engine:test-fast` run what the scan finds, and a new test joins them."""

    def test_the_filter_is_anchored_and_holds_the_files_that_have_tests(self):
        files = gate_vectors.swift_vector_tests()
        self.assertGreater(len(files), 20)
        pattern = re.compile(gate_vectors.swift_filter())
        for path in files:
            name = path.rsplit("/", 1)[1]
            with self.subTest(file=name):
                self.assertIsNotNone(pattern.search(f"SoyehtClientTests.aTest()/{name}:12:3"))
        self.assertIsNone(pattern.search("SoyehtClientTests.aTest()/PhoneWritingLayoutPolicyTests.swift:12:3"))
        self.assertNotIn("ContractVector", pattern.pattern)

    def test_every_swift_test_that_loads_a_vector_is_in_the_filter(self):
        in_filter = set(gate_vectors.swift_vector_tests())
        for path, text in gate_vectors.source_files():
            if path.startswith(gate_vectors.SWIFT_TESTS) and "@Test" in text and gate_vectors.VECTORS in text:
                with self.subTest(file=path):
                    self.assertIn(path, in_filter)

    def test_a_run_that_matches_too_few_tests_is_not_a_pass(self):
        self.assertEqual(gate_vectors.tests_run("􁁛  Test run with 324 tests in 0 suites passed after 1.9 seconds."), 324)
        self.assertEqual(gate_vectors.tests_run("Test run with 1 test in 0 suites passed"), 1)
        self.assertEqual(gate_vectors.tests_run("nothing"), 0)
        files = len(gate_vectors.swift_vector_tests())
        for output, status in ((f"Test run with {files} tests", 0), (f"Test run with {files - 1} tests", 1),
                               ("", 1)):
            run = mock.Mock(returncode=0, stdout=output, stderr="")
            with self.subTest(output=output), mock.patch("subprocess.run", return_value=run), \
                    contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(gate_vectors.swift_test(), status)

    def test_the_protocol_tests_are_the_targets_that_read_a_vector(self):
        self.assertLessEqual({"vectors", "contract", "client_policy", "version_range"}, set(gate_vectors.protocol_tests()))

    def test_the_steps_run_these_scripts(self):
        text = (ROOT / "scripts/check_all.sh").read_text()
        self.assertRegex(text, r"(?m)^step swift_vectors fast swift:test-shared-changed swift_vector_tests$")
        self.assertRegex(text, r"(?m)^step protocol_vectors fast engine:test-fast in_dir engine protocol_vector_tests$")
        self.assertIn("scripts/check_swift.sh --vectors && uv run python scripts/gate_vectors.py swift-test", text)
        self.assertIn("gate_vectors.py\" protocol-tests", text)


if __name__ == "__main__":
    unittest.main()
