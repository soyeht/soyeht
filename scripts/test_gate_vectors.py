import re
import unittest

import gate_plan
import gate_vectors
from gate_vectors import ROOT, VECTOR_READERS, readers_of, scan, vectors_read
from test_gate_plan import PACKAGES

NAMES = ["usage", "usage-presentation", "update", "markdown"]


class ReaderRuleTests(unittest.TestCase):
    """What counts as a read of a vector file."""

    def read(self, text: str) -> set[str]:
        return vectors_read(text, NAMES)

    def test_a_path_and_a_loader_call_are_reads(self):
        self.assertEqual(self.read('include_str!("../../contracts/vectors/usage.json")'), {"usage"})
        self.assertEqual(self.read('try ContractVector.load("update")'), {"update"})
        self.assertEqual(self.read('crate::testing::vector("usage-presentation")'), {"usage-presentation"})
        self.assertEqual(self.read('testing::cases("markdown", "runs")'), {"markdown"})

    def test_a_name_that_only_starts_like_another_is_not_it(self):
        self.assertEqual(self.read('vector("usage-presentation")'), {"usage-presentation"})
        self.assertEqual(self.read('contracts/vectors/usage-presentation.json'), {"usage-presentation"})

    def test_a_comment_or_a_key_of_the_same_name_is_not_a_read(self):
        self.assertEqual(self.read("// the cases of contracts/vectors/usage.json\n/// vector(\"update\")"), set())
        self.assertEqual(self.read('let key = "markdown"; vector_count()'), set())

    def test_a_file_name_counts_only_beside_the_word_vector(self):
        self.assertEqual(self.read('join("markdown.json")'), set())
        self.assertEqual(self.read('vectors_dir().join("markdown.json")'), {"markdown"})

    def test_a_reader_is_a_directory_of_the_table_or_an_engine_crate(self):
        for path, reader in (
            ("apps/shared/Tests/SoyehtClientTests/A.swift", "apps/shared"),
            ("apps/desktop/crates/soyeht-client/src/a.rs", "apps/desktop"),
            ("contracts/typescript/test/a.test.ts", "contracts/typescript"),
            ("engine/soyeht-avatar/src/tests/vectors/mod.rs", "engine/soyeht-avatar"),
            ("apps/mac/Sources/A.swift", None),
        ):
            with self.subTest(path=path):
                self.assertEqual(gate_vectors.reader_directory(path), reader)

    def test_only_a_json_file_directly_under_the_vectors_is_a_vector(self):
        self.assertEqual(readers_of("contracts/vectors/voice-turns.json"), ("apps/shared",))
        for path in ("contracts/vectors/new.json", "contracts/vectors/sub/voice-turns.json",
                     "contracts/vectors/voice-turns.md", "contracts/voice-turns.json"):
            with self.subTest(path=path):
                self.assertIsNone(readers_of(path))


class TableTests(unittest.TestCase):
    """The table says what the sources do: the plan skips the readers it does not list."""

    def test_every_vector_file_has_an_entry_and_every_entry_a_file(self):
        self.assertEqual(sorted(VECTOR_READERS), gate_vectors.vector_names())

    def test_each_entry_lists_exactly_the_directories_whose_sources_read_the_vector(self):
        problems = []
        for name, found in scan().items():
            declared = set(VECTOR_READERS.get(name, ()))
            if "unclassified" in found:
                problems.append(
                    f"{name}: read by {sorted(found['unclassified'])}, which no reader directory holds; "
                    "add the directory to READER_AREAS"
                )
            for reader in sorted(set(found) - declared - {"unclassified"}):
                problems.append(f"{name}: {sorted(found[reader])[0]} reads it; add {reader} to its entry")
            for reader in sorted(declared - set(found)):
                problems.append(f"{name}: no source of {reader} reads it any more; remove it from its entry")
        self.assertEqual(problems, [])

    def test_every_reader_is_a_known_directory_or_a_crate_of_the_engine(self):
        crates = {path.parent.relative_to(ROOT).as_posix() for path in ROOT.glob("engine/*/Cargo.toml")}
        for name, readers in VECTOR_READERS.items():
            for reader in readers:
                with self.subTest(vector=name, reader=reader):
                    self.assertTrue(reader in gate_vectors.READER_AREAS or reader in crates)
        for reader, area in gate_vectors.READER_AREAS.items():
            with self.subTest(reader=reader):
                self.assertTrue((ROOT / reader).is_dir())
                self.assertIn(area, gate_plan.ALL_AREAS)

    def test_no_entry_widens_the_plan_to_the_whole_workspace_or_the_end_to_end_suites(self):
        for name in VECTOR_READERS:
            result = gate_plan.make_plan([f"contracts/vectors/{name}.json"], PACKAGES, set())
            with self.subTest(vector=name):
                self.assertEqual(len(result.vectors), 1)
                self.assertNotIn("e2e", result.areas)
                if "engine" in result.areas:
                    self.assertIsNotNone(result.engine_crates)


class SwiftFilterTests(unittest.TestCase):
    """`swift:test-shared-changed` runs the Swift tests that read a vector, and a new one joins it."""

    def test_the_filter_matches_the_id_of_each_test_of_a_vector_file_and_no_other(self):
        files = gate_vectors.swift_vector_tests()
        self.assertGreater(len(files), 20)
        pattern = re.compile(gate_vectors.swift_filter())
        for path in files:
            name = path.rsplit("/", 1)[1]
            with self.subTest(file=name):
                self.assertIsNotNone(pattern.search(f"SoyehtClientTests.aTest()/{name}:12:3"))
        self.assertIsNone(pattern.search("SoyehtClientTests.aTest()/ComposerLayoutTests.swift:12:3"))

    def test_every_swift_test_that_loads_a_vector_is_in_the_filter(self):
        in_filter = set(gate_vectors.swift_vector_tests())
        for path, text in gate_vectors.source_files():
            code = gate_vectors.code_lines(text)
            if path.startswith(gate_vectors.SWIFT_VECTOR_TESTS) and "@Test" in code:
                if any(hint in code for hint in gate_vectors.LOADER_HINTS):
                    with self.subTest(file=path):
                        self.assertIn(path, in_filter)

    def test_the_step_runs_the_filter_this_script_prints(self):
        text = (ROOT / "scripts/check_all.sh").read_text()
        self.assertRegex(text, r"(?m)^step swift_vectors fast swift:test-shared-changed ")
        self.assertIn('--filter "$filter"', text)
        self.assertIn("scripts/gate_vectors.py swift-filter", text)


if __name__ == "__main__":
    unittest.main()
