"""Regression tests for the swallowed-error ratchet."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from check_swallowed_errors import classified, measure, sites, without_test_items
from quality_core import DUPLICATION_SCANS, SWALLOWED_KEY, differences


RUST = '''
fn production() -> Result<()> {
    let _ = cleanup();
    let value = parse().ok();
    let _ = validate(
        &input,
    )?;
    let text = "let _ = not code; .ok()";
    // let _ = a comment; .ok()
    let raw = r#"let _ = "raw" .ok()"#;
    match read() {
        Ok(value) => value,
        Err(error) => log(error),
        Err(_) => None,
    }
    match send() {
        Err(error) if error.is_closed() => (),
        Err(_) if retry => (),
        Err(_) if left == right => (),
        Err(_) => (),
        Ok(Err(_)) => (),
    }
    let text = "Err(_) => not code";
    Ok(())
}

#[cfg(test)]
mod tests {
    fn helper() {
        let _ = ignored();
        let _ = also().ok();
        match x { Err(_) => (), _ => () }
    }
}
'''
SWIFT = """
func load() {
  let a = try? decode()
  // try? in a comment
  let b = "try? in a string"
}
"""


def swallowed_changes(baseline: dict, actual: dict) -> list:
    """Only this metric's entries of the quality comparison."""
    empty = {
        "duplication": {
            area: {"clones": 0, "duplicated_lines": 0} for area in DUPLICATION_SCANS
        },
        "oversized_files": {},
        "loose_literals": {},
    }
    return differences({**empty, **baseline}, {**empty, **actual})


class SwallowedErrorTests(unittest.TestCase):
    def fixture(self) -> Path:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        (root / "engine/crate/src").mkdir(parents=True)
        (root / "engine/crate/tests").mkdir(parents=True)
        (root / "engine/crate/src/lib.rs").write_text(RUST)
        (root / "engine/crate/src/test_support.rs").write_text("let _ = x;\n")
        (root / "engine/crate/tests/it.rs").write_text("let _ = x.ok();\n")
        (root / "apps/desktop/crates/client/src").mkdir(parents=True)
        (root / "apps/desktop/crates/client/tests").mkdir(parents=True)
        (root / "apps/desktop/crates/client/src/lib.rs").write_text(
            "fn send() { let _ = tx.send(1).ok(); }\n"
            "#[cfg(test)]\nmod tests { fn t() { let _ = x.ok(); } }\n"
        )
        (root / "apps/desktop/crates/client/tests/it.rs").write_text("let _ = x.ok();\n")
        for area in ("apps/shared/Sources", "apps/mac/Sources", "apps/ios/Sources"):
            (root / area).mkdir(parents=True)
        (root / "apps/mac/Sources/View.swift").write_text(SWIFT)
        (root / "apps/shared/Tests").mkdir(parents=True)
        (root / "apps/shared/Tests/ViewTests.swift").write_text(SWIFT)
        return root

    def test_counts_only_production_code_outside_comments_strings_and_tests(self):
        root = self.fixture()
        self.assertEqual(
            measure(root),
            {
                "swift_try_optional": 1,
                "rust_ok": 1,
                "rust_let_underscore": 2,
                "rust_err_wildcard": 4,
            },
        )
        self.assertIn(("rust_err_wildcard", "engine/crate/src/lib.rs", 14), sites(root))
        self.assertIn(("rust_let_underscore", "engine/crate/src/lib.rs", 3), sites(root))
        self.assertIn(("rust_ok", "apps/desktop/crates/client/src/lib.rs", 1), sites(root))
        # `parse().ok()` is a conversion, not counted.
        self.assertNotIn(("rust_ok", "engine/crate/src/lib.rs", 4), sites(root))

    def exempt(self, source: str) -> list[tuple[str, int, str]]:
        """(metric, line, exemption) of every site in one production file."""
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        (root / "engine/crate/src").mkdir(parents=True)
        (root / "engine/crate/src/lib.rs").write_text(source)
        return [(metric, line, why) for metric, _, line, why in classified(root)]

    def test_ok_on_a_pure_conversion_is_not_counted(self):
        source = (
            "fn f() {\n"
            "    let a = text.parse::<u32>().ok();\n"
            "    let b = u32::try_from(value).ok()?;\n"
            "    let c = u64::from_str_radix(stem, 16)\n"
            "        .ok();\n"
            "    let d = String::from_utf8(bytes.to_vec()).ok();\n"
            "    let e = parts.next()?.parse().ok()?;\n"
            "    let f = serde_json::from_value::<Layout>(layout.clone()).ok()?;\n"
            "    let g = DateTime::parse_from_rfc3339(&(at.to_owned() + \"z\")).ok();\n"
            "}\n"
        )
        self.assertEqual(
            self.exempt(source),
            [("rust_ok", line, "conversion") for line in (2, 3, 5, 6, 7, 8, 9)],
        )

    def test_ok_on_io_or_on_something_that_only_ends_in_a_conversion_name_is_counted(self):
        source = (
            "fn f() {\n"
            "    let a = std::fs::read(path).ok();\n"
            "    let b = std::env::var(name).ok();\n"
            "    let c = connection.parse_all().ok();\n"
            "    let d = receiver.recv().ok().map(|v| v.parse::<u8>());\n"
            "    let e = (parse(x)).len().ok();\n"
            "    let f = parse;\n"
            "}\n"
        )
        self.assertEqual(
            self.exempt(source),
            [("rust_ok", line, "") for line in (2, 3, 4, 5, 6)],
        )

    def test_an_ignored_comment_with_a_reason_exempts_the_next_line_or_its_own(self):
        source = (
            "fn f() {\n"
            "    // ignored: the file may be gone\n"
            "    let _ = fs::remove_file(path);\n"
            "    let _ = fs::remove_file(other); // ignored: a gap is normal\n"
            "    // ignored: a long reason that\n"
            "    // continues on the next line\n"
            "    let _ = send();\n"
            "    let _ = after_block();\n"
            "    match r {\n"
            "        // ignored: the reason is the log line below\n"
            "        Err(_) => (),\n"
            "        Ok(_) => (),\n"
            "    }\n"
            "}\n"
        )
        self.assertEqual(
            self.exempt(source),
            [
                ("rust_let_underscore", 3, "ignored"),
                ("rust_let_underscore", 4, "ignored"),
                ("rust_let_underscore", 7, "ignored"),
                ("rust_let_underscore", 8, ""),
                ("rust_err_wildcard", 11, "ignored"),
            ],
        )

    def test_a_marker_needs_a_reason_and_speaks_only_for_what_it_precedes(self):
        source = (
            "fn f() {\n"
            "    // ignored:\n"
            "    let _ = a();\n"
            "    // ignored\n"
            "    let _ = b();\n"
            "    let _ = c(); // ignored: reason on c\n"
            "    let _ = d();\n"
            "    let x = \"// ignored: not a comment\";\n"
            "    let _ = e();\n"
            "}\n"
        )
        self.assertEqual(
            [why for _, _, why in self.exempt(source)],
            ["", "", "ignored", "", ""],
        )

    def test_an_ignored_comment_does_not_exempt_the_line_after_a_trailing_one(self):
        source = "fn f() {\n    g(); // ignored: g is optional\n    let _ = h();\n}\n"
        self.assertEqual(self.exempt(source), [("rust_let_underscore", 3, "")])

    def test_a_test_only_field_or_statement_does_not_hide_production_code_after_it(self):
        code = (
            "fn run(\n    #[cfg(test)] gate: Gate,\n) {\n    let _ = reply.send(1);\n"
            "    #[cfg(test)]\n    { let _ = ignored(); }\n"
            "    #[cfg(all(test, feature = \"test-support\"))]\n    let _ = also_ignored();\n"
            "    let _ = counted();\n}\n"
        )
        kept = without_test_items(code)
        self.assertEqual(kept.count("let _ ="), 2)
        self.assertIn("reply.send", kept)
        self.assertNotIn("ignored", kept)
        self.assertEqual(kept.count("\n"), code.count("\n"))

    def test_a_file_that_is_test_only_as_a_whole_is_blank(self):
        code = "#![cfg(test)]\nfn helper() { let _ = x(); }\n"
        self.assertNotIn("let", without_test_items(code))

    def test_the_quality_ratchet_holds_each_count_exactly(self):
        baseline = {SWALLOWED_KEY: {"swift_try_optional": 1, "rust_ok": 1}}
        actual = {SWALLOWED_KEY: {"swift_try_optional": 2, "rust_ok": 0}}
        changes = swallowed_changes(baseline, actual)
        self.assertEqual(
            [(change.direction, change.key) for change in changes],
            [
                ("IMPROVEMENT", "swallowed_errors.rust_ok"),
                ("RELAXATION", "swallowed_errors.swift_try_optional"),
            ],
        )
        # A baseline recorded before the metric existed compares nothing.
        self.assertEqual(swallowed_changes({}, actual), [])


if __name__ == "__main__":
    unittest.main()
