"""Regression tests for the untyped event literal ratchet."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from check_event_literals import KEY, main, sites

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
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        (root / "engine/soyeht-engine/src").mkdir(parents=True)
        (root / "engine/soyeht-engine/tests").mkdir(parents=True)
        (root / "engine/soyeht-store/src").mkdir(parents=True)
        (root / "engine/soyeht-engine/src/lib.rs").write_text(SOURCE)
        (root / "engine/soyeht-engine/src/fixtures.rs").write_text('fn f() { json!({"paneId": 1}); }\n')
        (root / "engine/soyeht-engine/tests/it.rs").write_text('fn t() { json!({"paneId": 1}); }\n')
        (root / "engine/soyeht-store/src/lib.rs").write_text('fn s() { json!({"paneId": 1}); }\n')
        return root

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

    def run_with(self, allowed: int) -> int:
        root = self.fixture()
        baseline = root / "baseline.json"
        baseline.write_text(json.dumps({KEY: allowed}))
        import sys

        arguments = sys.argv
        sys.argv = ["check", "--root", str(root), "--baseline", str(baseline)]
        self.addCleanup(setattr, sys, "argv", arguments)
        return main()

    def test_the_count_may_not_grow_and_the_baseline_follows_it_down(self):
        self.assertEqual(self.run_with(4), 0)
        self.assertEqual(self.run_with(3), 1)
        self.assertEqual(self.run_with(5), 1)


if __name__ == "__main__":
    unittest.main()
