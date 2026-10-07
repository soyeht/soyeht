import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import hardening_gate as gate
from gate_base import BaseError, resolve_base


def git(root, *arguments):
    return subprocess.run(
        [
            "git",
            "-c",
            "user.name=Gate Test",
            "-c",
            "user.email=gate@example.invalid",
            "-c",
            "commit.gpgsign=false",
            *arguments,
        ],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()


def commit(root, name):
    (Path(root) / name).write_text(name)
    git(root, "add", name)
    git(root, "commit", "--quiet", "--message", name)
    return git(root, "rev-parse", "HEAD")


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

    def function_lengths(self, path, source):
        result = gate.metrics(MemoryTree({path: source}))
        return [(item.name, item.lines) for item in result.functions]

    def test_swift_protocol_requirement_is_not_credited_with_the_next_type(self):
        body = "        work()\n" * 40
        source = (
            "protocol DictationTimer {\n"
            "    func cancel()\n"
            "}\n"
            "final class Timer: DictationTimer {\n"
            "    func cancel() {\n" + body + "    }\n"
            "}\n"
        )
        self.assertEqual(self.function_lengths("apps/shared/Timer.swift", source), [("cancel", 42)])

    def test_swift_bodyless_requirements_end_at_newline_brace_or_semicolon(self):
        source = (
            "protocol Port {\n"
            "    func one()\n"
            "    func two() throws\n"
            "    func three() -> Int where Self: Sendable\n"
            "    init(value: Int)\n"
            "    func four(); func five()\n"
            "}\n"
            "protocol Inline { func six() }\n"
            "struct Real {\n"
            "    func real() {\n        work()\n    }\n"
            "}\n"
        )
        self.assertEqual(self.function_lengths("apps/shared/Port.swift", source), [("real", 3)])

    def test_swift_signatures_that_span_lines_keep_their_body(self):
        source = (
            "struct Model {\n"
            "    @discardableResult\n"
            "    @MainActor\n"
            "    func multiline(\n"
            "        first: Int,\n"
            "        second: String\n"
            "    ) async throws -> Int {\n"
            "        work()\n"
            "    }\n"
            "    func constrained<T: Sequence>(items: T) -> [T.Element]\n"
            "        where T.Element: Hashable,\n"
            "              T: Sendable\n"
            "    {\n"
            "        work()\n"
            "    }\n"
            "    func trailingWhere<T>(items: T) -> Int where\n"
            "        T: Equatable {\n"
            "        work()\n"
            "    }\n"
            "    func arrow(callback: (Int) -> Void)\n"
            "        -> (Int,\n"
            "            String)\n"
            "    {\n"
            "        work()\n"
            "    }\n"
            "    func generic<T: Fn() -> Void>(_ value: T) { work() }\n"
            "}\n"
        )
        self.assertEqual(
            self.function_lengths("apps/shared/Model.swift", source),
            [("multiline", 6), ("constrained", 6), ("trailingWhere", 4), ("arrow", 6), ("generic", 1)],
        )

    def test_swift_closure_in_default_argument_does_not_open_the_body(self):
        source = (
            "protocol Loader {\n"
            "    func load(done: () -> Void = { work() })\n"
            "}\n"
            "struct Real {\n"
            "    func load(done: () -> Void = {\n        work()\n    }) {\n        run()\n    }\n"
            "}\n"
        )
        self.assertEqual(self.function_lengths("apps/shared/Loader.swift", source), [("load", 5)])

    def test_rust_signatures_with_semicolons_and_where_clauses_keep_their_body(self):
        source = (
            "trait Port {\n"
            "    fn load(&self) -> [u8; 4];\n"
            "}\n"
            "fn array(bytes: [u8; 4]) -> [u8; 2] {\n    work();\n}\n"
            "fn bounded<F>(call: F) -> u8\n"
            "where\n    F: Fn(u8) -> u8,\n{\n    work();\n}\n"
            "fn closure(call: impl Fn() -> u8) { work(); }\n"
        )
        self.assertEqual(
            self.function_lengths("engine/tool/src/lib.rs", source),
            [("array", 3), ("bounded", 6), ("closure", 1)],
        )

    def test_rust_lifetimes_do_not_hide_the_signature_from_the_scanner(self):
        source = (
            "fn matches<'t>(&'t self, text: &'t str) -> impl Iterator<Item = (usize, usize)> + 't {\n"
            "    work();\n}\n"
            "fn brace() -> char { '{' }\n"
        )
        self.assertEqual(
            self.function_lengths("engine/tool/src/lib.rs", source), [("matches", 3), ("brace", 1)]
        )

    def test_typescript_overloads_and_default_closures(self):
        source = (
            "function overload(x: number): void;\n"
            "function overload(x: string, cb = () => { work(); }): void {\n  work();\n}\n"
        )
        self.assertEqual(self.function_lengths("web/src/app.ts", source), [("overload", 3)])

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

    def test_gate_steps_count_uv_run_python_like_python3(self):
        source = (
            "uv run python -m unittest discover -s scripts\n"
            "uv run python scripts/check_quality.py\n"
            "python3 contracts/generate.py --check\n"
        )
        self.assertEqual(gate.count_gate_steps(source), 3)

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
                "// #[cfg_attr(not(feature = \"fast\"), ignore)]\n"
                "#[test]\n#[cfg_attr(not(feature = \"fast\"), ignore)]\nfn conditional() {}\n"
            ),
        }))
        self.assertEqual(result.tests, 2)
        self.assertEqual(result.ignored_tests, 2)

    def test_conditional_ignore_increase_trips_verification_ratchet(self):
        base = gate.metrics(MemoryTree({
            "engine/tool/src/lib.rs": "#[test]\nfn conditional() {}\n",
        }))
        head = gate.metrics(MemoryTree({
            "engine/tool/src/lib.rs": (
                "#[test]\n"
                "#[cfg_attr(not(feature = \"fast\"), ignore)]\n"
                "fn conditional() {}\n"
            ),
        }))
        self.assertEqual((base.ignored_tests, head.ignored_tests), (0, 1))
        self.assertTrue(gate.check_verification(base, head, False))


class HardeningBaseTests(unittest.TestCase):
    def test_default_base_is_the_merge_base_with_origin_main_not_head(self):
        with tempfile.TemporaryDirectory() as root:
            git(root, "init", "--quiet", "--initial-branch=main")
            fork = commit(root, "fork")
            commit(root, "main-only")
            git(root, "update-ref", "refs/remotes/origin/main", "HEAD")
            git(root, "switch", "--quiet", "--create", "feature", fork)
            head = commit(root, "feature")
            base = resolve_base(Path(root))
            self.assertEqual(base, fork)
            self.assertNotEqual(base, head)

    def test_missing_origin_main_fails_instead_of_comparing_with_head(self):
        with tempfile.TemporaryDirectory() as root:
            git(root, "init", "--quiet")
            commit(root, "only")
            with self.assertRaisesRegex(BaseError, "merge-base of origin/main"):
                resolve_base(Path(root))

    def test_explicit_base_must_be_a_full_oid(self):
        self.assertEqual(resolve_base(Path("."), "a" * 40), "a" * 40)
        with self.assertRaisesRegex(BaseError, "full commit OID"):
            resolve_base(Path("."), "main")

    def test_main_measures_the_resolved_base(self):
        resolved = "b" * 40
        empty = gate.Metrics(frozenset(), frozenset(), 0, 0, (), 0, 0, 0, 0)
        with (
            patch.object(sys, "argv", ["hardening_gate.py", "check-no-panic-in-production"]),
            patch.object(gate, "resolve_base", return_value=resolved) as resolve,
            patch.object(gate, "metrics", return_value=empty),
            patch.object(gate, "CommitTree") as tree,
        ):
            self.assertEqual(gate.main(), 0)
        resolve.assert_called_once_with(gate.ROOT, None)
        tree.assert_called_once_with(resolved)


if __name__ == "__main__":
    unittest.main()
