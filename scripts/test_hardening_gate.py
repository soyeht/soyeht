import tempfile
import unittest
from pathlib import Path

import hardening_gate as gate


class MemoryTree:
    def __init__(self, files):
        self.files = files

    def paths(self):
        return sorted(self.files)

    def read(self, path):
        return self.files[path]


class HardeningGateTests(unittest.TestCase):
    def test_sql_check_reads_literals_but_not_comments_or_ui_copy(self):
        tree = MemoryTree({
            "engine/soyeht-engine/Cargo.toml": "rusqlite.workspace = true\n",
            "engine/soyeht-engine/src/live.rs": (
                '// "DELETE FROM panes"\n'
                'let label = "Select a pane";\n'
                'let query = r#"SELECT payload FROM panes"#;\n'
            ),
            "engine/soyeht-store/src/database.rs": 'let sql = "SELECT x FROM y";\n',
        })
        result = gate.metrics(tree)
        self.assertEqual(result.sql_manifests, {"engine/soyeht-engine/Cargo.toml"})
        self.assertEqual(result.sql_sources, {"engine/soyeht-engine/src/live.rs"})
        self.assertEqual(result.sql_statements, 1)

    def test_panic_check_excludes_cfg_test_modules_and_test_directories(self):
        tree = MemoryTree({
            "engine/soyeht-engine/src/live.rs": """
fn live(value: Option<u8>) { value.expect("required"); }
#[cfg(test)]
mod tests { #[test] fn unit() { panic!("red control"); } }
""",
            "engine/soyeht-engine/tests/runtime.rs": "#[test] fn external() { None.unwrap(); }",
        })
        result = gate.metrics(tree)
        self.assertEqual(result.panic_calls, 1)
        self.assertEqual(result.tests, 2)

    def test_function_lengths_cover_braced_and_indented_languages(self):
        rust = "fn long() {\n" + "let x = 1;\n" * 121 + "}\n"
        python = "def short():\n    return 1\n"
        result = gate.metrics(MemoryTree({
            "engine/tool/src/lib.rs": rust,
            "scripts/tool.py": python,
        }))
        lengths = {function.name: function.lines for function in result.functions}
        self.assertEqual(lengths["long"], 123)
        self.assertEqual(lengths["short"], 2)

    def test_generated_functions_do_not_enter_the_length_inventory(self):
        source = "// Generated from a reviewed schema. Do not edit.\nfn huge() {\n" + "x();\n" * 250 + "}\n"
        result = gate.metrics(MemoryTree({"contracts/generated.rs": source}))
        self.assertEqual(result.functions, ())

    def test_bodyless_trait_methods_are_not_attached_to_the_next_body(self):
        source = "trait Port { fn load(&self) -> Result<()>; }\nfn real() { work(); }\n"
        result = gate.metrics(MemoryTree({"engine/tool/src/lib.rs": source}))
        self.assertEqual([(item.name, item.lines) for item in result.functions], [("real", 1)])

    def test_panic_inventory_covers_every_production_crate(self):
        result = gate.metrics(MemoryTree({
            "engine/soyeht-store/src/lib.rs": "fn store() { None.unwrap(); }",
            "engine/soyeht-ptyd/src/lib.rs": "fn supervisor() { panic!(\"fault\"); }",
        }))
        self.assertEqual(result.panic_calls, 2)

    def test_verification_drop_requires_an_explicit_reason_line(self):
        base = gate.Metrics(frozenset(), frozenset(), 0, 0, (), 5, 16, 12, 1)
        head = gate.Metrics(frozenset(), frozenset(), 0, 0, (), 4, 16, 12, 1)
        self.assertTrue(gate.check_verification(base, head, False))
        self.assertFalse(gate.check_verification(base, head, True))

        ignored = gate.Metrics(frozenset(), frozenset(), 0, 0, (), 5, 16, 12, 2)
        self.assertTrue(gate.check_verification(base, ignored, False))
        self.assertFalse(gate.check_verification(base, ignored, True))

        with tempfile.TemporaryDirectory() as directory:
            event = Path(directory) / "event.json"
            event.write_text('{"pull_request":{"body":"Verification change: replaced a duplicate test"}}')
            self.assertTrue(gate.waiver(event))
            event.write_text('{"pull_request":{"body":"No verification change requested"}}')
            self.assertFalse(gate.waiver(event))

    def test_comments_cannot_pad_verification_counts(self):
        result = gate.metrics(MemoryTree({
            "engine/tool/src/lib.rs": '// #[test]\n#[test]\nfn real() {}\n',
            "apps/shared/Tests/Client.swift": '// @Test func fake() {}\n@Test func real() {}\n',
            "scripts/e2e.py": (
                '# results.append({"name": "fake", "passed": True})\n'
                'results.append({"name": "real", "passed": True})\n'
            ),
            "scripts/check_all.sh": "# cargo test\ncargo test --locked\n",
        }))
        self.assertEqual(result.tests, 2)
        self.assertEqual(result.ignored_tests, 0)
        self.assertEqual(result.acceptances, 1)
        self.assertEqual(result.gate_steps, 1)

    def test_ratchets_reject_new_sql_panics_and_long_functions(self):
        empty = gate.Metrics(frozenset(), frozenset(), 0, 0, (), 0, 0, 0, 0)
        long = gate.Function("engine/a.rs", "large", 1, 201)
        head = gate.Metrics(
            frozenset({"engine/Cargo.toml"}),
            frozenset(),
            0,
            1,
            (long,),
            0,
            0,
            0,
            0,
        )
        self.assertTrue(gate.check_sql(empty, head))
        self.assertTrue(gate.check_panic(empty, head))
        self.assertTrue(gate.check_lengths(empty, head))

    def test_ignored_rust_tests_are_counted_without_comment_padding(self):
        result = gate.metrics(MemoryTree({
            "engine/tool/src/lib.rs": (
                "// #[ignore]\n"
                "#[test]\n#[ignore = \"measured separately\"]\nfn isolated() {}\n"
            ),
        }))
        self.assertEqual(result.tests, 1)
        self.assertEqual(result.ignored_tests, 1)


if __name__ == "__main__":
    unittest.main()
