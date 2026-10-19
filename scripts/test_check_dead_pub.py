"""Regression tests for the dead public function check."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from check_dead_pub import candidates, problems

LIBRARY = '''
pub fn used() {}
pub fn dead() {}
pub async fn dead_async() {}
pub const fn dead_const() {}
pub fn only_tests_call_me() {}
pub fn only_a_comment_names_me() {}
pub fn only_a_string_names_me() {}
pub fn called_from_desktop() {}
pub fn serialize() {}
pub(crate) fn crate_private() {}
fn private() {}
// only_a_comment_names_me
const NAME: &str = "only_a_string_names_me";

pub fn caller() {
    used();
}

#[cfg(test)]
mod tests {
    pub fn defined_in_tests() {}
    fn t() {
        only_tests_call_me();
    }
}

#[cfg(all(test, feature = "test-support"))]
fn also_test_only() {
    only_tests_call_me();
}

struct Probe {
    #[cfg(test)]
    counter: u32,
}

fn after_a_cfg_test_field() {
    called_later();
}
'''


class DeadPubTests(unittest.TestCase):
    def fixture(self) -> Path:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        for relative in ("engine/crate/src", "engine/crate/tests", "apps/desktop/src"):
            (root / relative).mkdir(parents=True)
        (root / "engine/crate/src/lib.rs").write_text(LIBRARY)
        (root / "engine/crate/src/testing.rs").write_text("pub fn test_helper() {}\n")
        (root / "engine/crate/tests/it.rs").write_text("fn t() { dead(); pub fn x() {} }\n")
        (root / "apps/desktop/src/main.rs").write_text("fn main() { called_from_desktop(); }\n")
        return root

    def test_lists_only_pub_fns_no_production_code_references(self):
        names = [name for _, _, name in candidates(self.fixture())]
        self.assertEqual(
            names,
            [
                "dead",
                "dead_async",
                "dead_const",
                "only_tests_call_me",
                "only_a_comment_names_me",
                "only_a_string_names_me",
                "caller",
            ],
        )

    def test_a_pub_fn_of_the_desktop_that_only_tests_call_is_listed(self):
        root = self.fixture()
        crate = root / "apps/desktop/crates/client/src"
        crate.mkdir(parents=True)
        (crate / "lib.rs").write_text(
            "pub fn used_by_the_window() {}\n"
            "pub fn desktop_dead() {}\n"
            "pub fn caller() { used_by_the_window(); }\n"
            "#[cfg(test)]\nmod tests { fn t() { super::desktop_dead(); } }\n"
        )
        found = [(path, name) for path, _, name in candidates(root) if "apps/desktop" in path]
        self.assertEqual(
            found,
            [
                ("apps/desktop/crates/client/src/lib.rs", "desktop_dead"),
                ("apps/desktop/crates/client/src/lib.rs", "caller"),
            ],
        )

    def test_reports_the_line_of_the_definition(self):
        found = {name: line for _, line, name in candidates(self.fixture())}
        self.assertEqual(found["dead"], 3)

    def test_a_test_item_after_a_cfg_test_field_does_not_hide_production_code(self):
        root = self.fixture()
        (root / "engine/crate/src/lib.rs").write_text(
            "pub fn called_later() {}\n"
            "struct Probe {\n    #[cfg(test)]\n    counter: u32,\n}\n"
            "fn caller() { called_later(); }\n"
        )
        self.assertEqual(candidates(root), [])

    def test_generated_code_counts_as_a_reference(self):
        root = self.fixture()
        (root / "engine/crate/src/generated.rs").write_text("fn g() { dead(); }\n")
        names = [name for _, _, name in candidates(root)]
        self.assertNotIn("dead", names)

    def test_the_allowlist_keeps_a_reviewed_entry_and_rejects_stale_ones(self):
        root = self.fixture()
        everything = {
            f"engine/crate/src/lib.rs::{name}": "reason"
            for _, _, name in candidates(root)
        }
        self.assertEqual(problems(root, everything), [])
        stale = {**everything, "engine/crate/src/lib.rs::gone": "reason"}
        self.assertEqual(
            problems(root, stale),
            ["engine/crate/src/lib.rs::gone: allowlist entry matches no dead pub fn; remove it"],
        )
        blank = {**everything, "engine/crate/src/lib.rs::dead": " "}
        self.assertEqual(
            problems(root, blank),
            ["engine/crate/src/lib.rs::dead: allowlist entry needs a reason"],
        )

    def test_an_unlisted_candidate_names_the_fix(self):
        messages = problems(self.fixture(), {})
        self.assertEqual(len(messages), 7)
        self.assertTrue(messages[0].startswith("engine/crate/src/lib.rs:3: pub fn dead "))

    def test_the_repository_has_no_dead_pub_fn(self):
        from check_dead_pub import ALLOWLIST
        from dead_declarations import ROOT
        import json

        allowlist = json.loads(ALLOWLIST.read_text(encoding="utf-8"))
        self.assertEqual(problems(ROOT, allowlist), [])


if __name__ == "__main__":
    unittest.main()
