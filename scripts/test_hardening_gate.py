import contextlib
import dataclasses
import io
import json
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock
from unittest.mock import patch

import forbidden_terms
import hardening_gate as gate
import verification_gate
from gate_base import BaseError, resolve_base
from verification_gate import Waivers


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


def step(area, tier, step_id, command="cargo test"):
    return (area, tier, step_id, command)


def metrics_with(**fields):
    return gate.Metrics(frozenset(), frozenset(), 0, 0, (), **fields)


def verdict(base, head, waived=Waivers()):
    """(failed, stdout, stderr) of the verification check."""
    shown, errors = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(shown), contextlib.redirect_stderr(errors):
        failed = gate.check_verification(base, head, waived)
    return failed, shown.getvalue(), errors.getvalue()


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

    def test_sql_exception_is_only_the_foreign_database_reader(self):
        query = 'let q = "SELECT value FROM ItemTable";\n'
        tree = MemoryTree({
            "engine/soyeht-usage/Cargo.toml": "rusqlite = { version = \"0.40\" }\n",
            "engine/soyeht-usage/src/foreign_db.rs": query,
            "engine/soyeht-usage/src/planner.rs": query,
            "engine/soyeht-engine/Cargo.toml": "rusqlite = { version = \"0.40\" }\n",
        })
        result = gate.metrics(tree)
        self.assertEqual(result.sql_manifests, {"engine/soyeht-engine/Cargo.toml"})
        self.assertEqual(result.sql_sources, {"engine/soyeht-usage/src/planner.rs"})

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
        self.assertEqual(len(result.tests), 2)

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

    def test_panic_inventory_covers_the_desktop_client_but_not_its_tests(self):
        result = gate.metrics(MemoryTree({
            "apps/desktop/crates/soyeht-client/src/state.rs": "fn s() { None.unwrap(); }",
            "apps/desktop/crates/soyeht-client/tests/engine.rs": "fn t() { None.unwrap(); }",
            "apps/desktop/crates/soyeht-client/src/other.rs": (
                "#[cfg(test)]\nmod tests { fn t() { None.unwrap(); } }\n"
            ),
        }))
        self.assertEqual(result.panic_calls, 1)

    def test_panic_inventory_covers_the_android_core_but_not_its_tests(self):
        result = gate.metrics(MemoryTree({
            "apps/android/core/src/core.rs": "fn c() { None.unwrap(); }",
            "apps/android/core/src/bin/uniffi-bindgen.rs": "fn main() { None::<u8>.unwrap(); }",
            "apps/android/core/tests/core.rs": "fn t() { None.unwrap(); }",
        }))
        self.assertEqual(result.panic_calls, 2)

    def waived(self, body):
        with tempfile.TemporaryDirectory() as directory:
            event = Path(directory) / "event.json"
            event.write_text(json.dumps({"pull_request": {"body": body}}))
            with contextlib.redirect_stderr(io.StringIO()):
                return gate.waivers(event)

    def test_a_waiver_names_what_it_waives_and_gives_a_reason(self):
        waived = self.waived(
            "Verification change: removes step e2e:old because a newer step covers its flow\n"
            "  Verification change: removes acceptance `old_flow` because the flow was removed\n"
            "Verification change: ignores slow_test because it needs the VM\n"
            "Verification change: none\n"
            "Verification change: removes step e2e:other because no\n"
            "Verification change: dropped a duplicate test because it was a duplicate\n"
        )
        self.assertEqual(waived, Waivers(frozenset({"slow_test"}), frozenset({"e2e:old"}), frozenset({"old_flow"})))
        self.assertEqual(self.waived("No verification change requested"), Waivers())

    def test_each_weakening_fails_alone_and_only_a_line_that_names_it_waives_it(self):
        weakenings = (  # (base, head, what the gate says, the waiver that names it)
            (dict(steps=(step("engine", "both", "a:one"),)), dict(steps=()),
             "step a:one was removed", Waivers(steps=frozenset({"a:one"}))),
            (dict(steps=(step("engine", "both", "a:one"),)), dict(steps=(step("engine", "full", "a:one"),)),
             "step a:one left the fast tier", Waivers(steps=frozenset({"a:one"}))),
            (dict(steps=(step("engine", "both", "a:one"),)), dict(steps=(step("engine", "fast", "a:one"),)),
             "step a:one left the full tier", Waivers(steps=frozenset({"a:one"}))),
            (dict(steps=(step("engine", "both", "a:one"),)), dict(steps=(step("android_core", "both", "a:one"),)),
             "step a:one moved from area engine to android_core", Waivers(steps=frozenset({"a:one"}))),
            (dict(steps=(step("always", "both", "a:one"),)), dict(steps=(step("engine", "both", "a:one"),)),
             "step a:one moved from area always to engine", Waivers(steps=frozenset({"a:one"}))),
            (dict(acceptances=("flow",)), dict(acceptances=()),
             "acceptance flow was removed", Waivers(acceptances=frozenset({"flow"}))),
            (dict(ignored_tests=("a.rs::gone",)), dict(ignored_tests=("a.rs::slow",)),
             "test a.rs::slow is ignored", Waivers(ignored_tests=frozenset({"slow"}))),
            (dict(ignored_tests=()), dict(ignored_tests=("a.rs::slow",)),
             "test a.rs::slow is ignored", Waivers(ignored_tests=frozenset({"a.rs::slow"}))),
        )
        elsewhere = Waivers(frozenset({"gone", "other"}), frozenset({"other"}), frozenset({"other"}))
        for before, after, message, naming in weakenings:
            with self.subTest(message):
                base, head = metrics_with(**before), metrics_with(**after)
                failed, shown, errors = verdict(base, head)
                self.assertTrue(failed)
                self.assertIn(message, errors)
                self.assertTrue(verdict(base, head, elsewhere)[0], "a line for another thing waived it")
                failed, shown, errors = verdict(base, head, naming)
                self.assertFalse(failed)
                self.assertIn(f"{message} (waived)", shown)

    def test_a_revert_waives_every_weakening_it_causes_and_nothing_else(self):
        base = metrics_with(steps=(step("engine", "both", "a:one"), step("engine", "both", "b:two")),
                            acceptances=("flow", "other"), ignored_tests=("a.rs::old",))
        head = metrics_with(steps=(step("engine", "full", "a:one"), step("engine", "both", "b:two")),
                            acceptances=("other",), ignored_tests=("a.rs::old", "a.rs::slow"))
        lines = verification_gate.revert_lines(base, head, "the revert restores it")
        self.assertEqual(lines, [
            "Verification change: removes acceptance flow because the revert restores it",
            "Verification change: removes step a:one because the revert restores it",
            "Verification change: ignores a.rs::slow because the revert restores it",
        ])
        self.assertEqual(verification_gate.revert_lines(base, base, "the revert restores it"), [])

    def test_a_name_the_message_scan_refuses_is_written_and_accepted_as_its_id(self):
        base = metrics_with(acceptances=("zzz_flow",))
        head = metrics_with()
        with mock.patch.object(forbidden_terms, "MESSAGE_PATTERNS", ("zzz",)):
            (line,) = verification_gate.revert_lines(base, head, "the revert restores it")
            identifier = verification_gate.short_id("zzz_flow")
            self.assertEqual(line, f"Verification change: removes acceptance {identifier} because the revert restores it")
            failed, _, errors = verdict(base, head)
            self.assertIn(f"acceptance zzz_flow (id {identifier}) was removed", errors)
            self.assertFalse(verdict(base, head, Waivers(acceptances=frozenset({identifier})))[0])
            self.assertTrue(verdict(base, head, Waivers(acceptances=frozenset({"zzz_other"})))[0])

    def test_a_step_that_gets_stronger_or_changes_its_command_is_not_a_weakening(self):
        base = metrics_with(steps=(step("engine", "full", "a:one"), step("engine", "both", "b:two", "cargo fmt")))
        head = metrics_with(steps=(step("always", "both", "a:one"), step("engine", "both", "b:two", "cargo test")))
        failed, shown, errors = verdict(base, head)
        self.assertEqual((failed, errors), (False, ""))
        self.assertIn("step b:two changed its command", shown)

    def test_deleting_tests_is_free_and_only_the_deleted_ones_are_printed(self):
        base = metrics_with(tests=("a.rs::kept", "a.rs::gone"))
        failed, shown, errors = verdict(base, metrics_with(tests=("a.rs::kept",)))
        self.assertEqual((failed, errors), (False, ""))
        self.assertIn("removed test a.rs::gone", shown)
        self.assertNotIn("removed test a.rs::kept", shown)

    def test_ignored_tests_are_the_rust_functions_an_ignore_applies_to(self):
        source = (
            "// #[ignore]\n#[test]\n#[ignore = \"measured separately\"]\nfn isolated() {}\n"
            "#[cfg_attr(not(feature = \"fast\"), ignore)]\n#[test]\npub async fn conditional() {}\n"
            "#[test]\nfn live() {}\n"
        )
        self.assertEqual(gate.ignored_test_names("a.rs", source), ["isolated", "conditional"])
        self.assertEqual(gate.ignored_test_names("a.py", source), [])

    def test_test_names_are_read_from_the_markers_of_each_language(self):
        names = gate.names_of_tests
        self.assertEqual(names("a.rs", "// #[test]\n#[test]\n#[ignore]\nasync fn one() {}\n#[tokio::test]\nfn two() {}"),
                         ["one", "two"])
        self.assertEqual(names("T.swift", '// @Test func fake() {}\n@Test("shown") func real() {}\n'), ["real"])
        self.assertEqual(names("t.py", "def test_a(): pass\nclass C:\n    def test_b(self): pass\ndef helper(): pass"),
                         ["test_a", "test_b"])
        self.assertEqual(names("x.test.ts", "// it('no', f)\ntest('yes', f)\nit(\"also\", f)"), ["yes", "also"])
        self.assertEqual(names("T.kt", "@Test\nfun works() {}"), ["works"])

    def test_gate_steps_are_the_ids_of_the_step_lines(self):
        source = (
            "step always both a:one uv run python x.py\n"
            "step engine full engine:test cargo test\n"
            "# step engine both commented cargo test\n"
            "cargo test --locked\n"
        )
        self.assertEqual(gate.gate_steps(source), (
            ("always", "both", "a:one", "uv run python x.py"), ("engine", "full", "engine:test", "cargo test"),
        ))

    def test_comments_cannot_pad_verification_counts(self):
        result = gate.metrics(MemoryTree({
            "engine/tool/src/lib.rs": '// #[test]\n#[test]\nfn real() {}\n',
            "apps/shared/Tests/Client.swift": '// @Test func fake() {}\n@Test func real() {}\n',
            "scripts/e2e.py": (
                '# results.append({"name": "fake", "passed": True})\n'
                'results.append({"name": "real", "passed": True})\n'
                'results.append({"name": f"Real {x}", "passed": True})\n'
            ),
            "scripts/check_all.sh": "# step always both fake:one cargo test\nstep always both real:one cargo test\n",
        }))
        self.assertEqual(result.tests, ("apps/shared/Tests/Client.swift::real", "engine/tool/src/lib.rs::real"))
        self.assertEqual(result.ignored_tests, ())
        self.assertEqual(result.acceptances, ("f'Real {x}'", "real"))
        self.assertEqual(result.steps, (("always", "both", "real:one", "cargo test"),))

    def test_ratchets_reject_new_sql_panics_and_long_functions(self):
        empty = gate.Metrics(frozenset(), frozenset(), 0, 0, ())
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
        self.assertEqual(len(result.tests), 2)
        self.assertEqual(len(result.ignored_tests), 2)

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
        self.assertEqual((base.ignored_tests, head.ignored_tests), ((), ("engine/tool/src/lib.rs::conditional",)))
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertTrue(gate.check_verification(base, head, Waivers()))


class MainWaiverTests(unittest.TestCase):
    def run_main(self, body):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "repo"
            (root / "scripts").mkdir(parents=True)
            (root / "scripts/check_all.sh").write_text("step always both a:one echo one\n")
            git(root, "init", "--quiet")
            git(root, "add", "-A")
            git(root, "commit", "--quiet", "--message", "base")
            base = git(root, "rev-parse", "HEAD")
            (root / "scripts/check_all.sh").write_text("")
            event = Path(directory) / "event.json"
            event.write_text(json.dumps({"pull_request": {"body": body}}))
            arguments = ["hardening_gate.py", "check-verification-not-weakened", "--base", base,
                         "--event-file", str(event)]
            with (patch.object(gate, "ROOT", root), patch.object(sys, "argv", arguments),
                  contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO())):
                return gate.main()

    def test_the_gate_honours_a_waiver_from_the_event_file_and_only_that_one(self):
        self.assertEqual(self.run_main("nothing to waive"), 1)
        self.assertEqual(self.run_main("Verification change: removes step a:other because it moved elsewhere"), 1)
        self.assertEqual(self.run_main("Verification change: removes step a:one because it moved elsewhere"), 0)


LARGE_FILE_FUNCTIONS = 1_000
LARGE_FILE_CPU_SECONDS = 2.0


def large_sources():
    """Four files of about 20k lines each, dense with functions, literals and comments."""
    rust = "".join(
        f"/// doc {n}\n"
        f"pub fn item_{n}<'a>(value: &'a str) -> Result<&'a str, Error> {{\n"
        f"    // comment {n}\n"
        f"    let text = \"literal {n}\"; let c = '{{'; /* block */\n"
        + "    work(value);\n" * 12
        + "    Ok(value)\n}\n"
        for n in range(LARGE_FILE_FUNCTIONS)
    )
    swift = "".join(
        f"func item{n}(value: String) -> String {{\n"
        f"    // comment {n}\n"
        f"    let text = \"literal {n}\"\n" + "    work(value)\n" * 13 + "    return text\n}\n"
        for n in range(LARGE_FILE_FUNCTIONS)
    )
    typescript = "".join(
        f"function item{n}(value: string): string {{\n"
        f"  // comment {n}\n"
        f"  const text = 'literal {n}';\n" + "  work(value);\n" * 13 + "  return text;\n}\n"
        for n in range(LARGE_FILE_FUNCTIONS)
    )
    python = "".join(
        f"def item_{n}(value):\n    # comment {n}\n    text = 'literal {n}'\n"
        + "    work(value)\n" * 13
        + "    return text\n\n"
        for n in range(LARGE_FILE_FUNCTIONS)
    )
    return {
        "engine/big/src/lib.rs": rust,
        "apps/big/Big.swift": swift,
        "tools/big/index.ts": typescript,
        "scripts/big.py": python,
    }


class HardeningScanSpeedTests(unittest.TestCase):
    def test_scanning_large_files_is_linear(self):
        sources = large_sources()
        for source in sources.values():
            self.assertGreaterEqual(source.count("\n"), 15_000)
        started = time.process_time()
        result = gate.metrics(MemoryTree(sources))
        elapsed = time.process_time() - started
        counts = {path: 0 for path in sources}
        for function in result.functions:
            counts[function.path] += 1
        self.assertEqual(counts, {path: LARGE_FILE_FUNCTIONS for path in sources})
        self.assertLess(
            elapsed, LARGE_FILE_CPU_SECONDS, f"scanning large files took {elapsed:.1f}s of CPU"
        )

    def test_mask_keeps_offsets_and_blanks_comments_and_literals(self):
        text = "a // c\nb /* x\ny */ \"s\\\"t\" r#\"q\"# 'x' <'a>\n"
        clean, strings = gate.mask(text, lifetimes=True)
        self.assertEqual(len(clean), len(text))
        self.assertEqual(clean.count("\n"), text.count("\n"))
        self.assertEqual(clean.split(), ["a", "b", "<'a>"])
        self.assertEqual(strings, ['s\\"t', "q"])

    def test_brace_pairs_matches_nested_and_ignores_unmatched_braces(self):
        self.assertEqual(gate.brace_pairs("{ { } } }"), {0: 6, 2: 4})
        self.assertEqual(gate.brace_pairs("{ {"), {})


class CommitTreeTests(unittest.TestCase):
    def test_commit_tree_reads_every_file_like_git_show(self):
        with tempfile.TemporaryDirectory() as root:
            git(root, "init", "--quiet", "--initial-branch=main")
            files = {
                "src/a.rs": "fn a() {}\n",
                "src/copy of a.rs": "fn a() {}\n",
                "empty.txt": "",
                "notes/ünïcode.md": "café\n",
            }
            for name, content in files.items():
                target = Path(root) / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(content, encoding="utf-8")
            (Path(root) / "blob.bin").write_bytes(bytes(range(256)))
            git(root, "add", "--all")
            git(root, "commit", "--quiet", "--message", "files")
            revision = git(root, "rev-parse", "HEAD")
            with patch.object(gate, "ROOT", Path(root)):
                tree = gate.CommitTree(revision)
                self.assertEqual(tree.paths(), sorted([*files, "blob.bin"]))
                for name in tree.paths():
                    expected = subprocess.check_output(
                        ["git", "show", f"{revision}:{name}"], cwd=root
                    ).decode(errors="replace")
                    self.assertEqual(tree.read(name), expected, name)
                with self.assertRaises(OSError):
                    tree.read("missing.rs")


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
        empty = gate.Metrics(frozenset(), frozenset(), 0, 0, ())
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
