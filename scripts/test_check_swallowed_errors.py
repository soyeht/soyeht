"""Regression tests for the swallowed-error ratchet."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from check_swallowed_errors import measure, sites, without_test_items
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
                "rust_ok": 2,
                "rust_let_underscore": 2,
                "rust_err_wildcard": 4,
            },
        )
        self.assertIn(("rust_err_wildcard", "engine/crate/src/lib.rs", 14), sites(root))
        self.assertIn(("rust_let_underscore", "engine/crate/src/lib.rs", 3), sites(root))
        self.assertIn(("rust_ok", "apps/desktop/crates/client/src/lib.rs", 1), sites(root))

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
