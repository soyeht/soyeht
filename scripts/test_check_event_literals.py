"""Regression tests for the untyped event literal ratchet."""

from __future__ import annotations

import unittest
from pathlib import Path

from check_event_literals import KEY, main, sites
from count_ratchet_testing import fixture_root, run_main

SOURCE = '''
fn events() {
    let one = json!({"paneId": id, "state": state});
    let two = json!({
        "paneId": id,
    });
    let other = json!({"workspaceId": id, "paneId": id});
    let nested = json!({"items": [{"paneId": id}]});
    let none = json!({"workspaceId": id});
    let text = "json!({\\"paneId\\": 1})";
    // json!({"paneId": 1})
}

#[cfg(test)]
mod fixtures;

#[cfg(test)]
mod tests {
    fn helper() {
        let skipped = json!({"paneId": "pane-1"});
    }
}
'''


class EventLiteralTests(unittest.TestCase):
    def fixture(self) -> Path:
        snippet = 'fn s() { json!({"paneId": 1}); }\n'
        return fixture_root(self, SOURCE, snippet, snippet)

    def test_counts_literals_with_a_pane_id_key_at_any_depth_in_engine_production_code(self):
        self.assertEqual(
            sites(self.fixture()),
            [
                ("engine/soyeht-engine/src/lib.rs", 3),
                ("engine/soyeht-engine/src/lib.rs", 4),
                ("engine/soyeht-engine/src/lib.rs", 7),
                ("engine/soyeht-engine/src/lib.rs", 8),
            ],
        )

    def test_the_count_may_not_grow_and_the_baseline_follows_it_down(self):
        for allowed, expected in ((4, 0), (3, 1), (5, 1)):
            with self.subTest(allowed=allowed):
                root = self.fixture()
                self.assertEqual(run_main(self, main, root, KEY, allowed), expected)


if __name__ == "__main__":
    unittest.main()
