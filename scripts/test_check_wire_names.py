"""Regression tests for the hand-typed wire name check."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from check_wire_names import sites

SCHEMA = {
    "x-methods": {"hello": {}, "pane.list": {}, "task.show": {}},
    "x-events": {"pane.updated": {}},
}
RUST = '''
use soyeht_protocol::method;

fn production() {
    let ok = method::PANE_LIST;
    let typed = "pane.list";
    let event = json!({"method": "pane.updated"});
    let plain = "hello";
    let other = "pane.listing";
    // "task.show" in a comment
}

#[cfg(test)]
mod tests {
    fn helper() {
        let typed = "pane.list";
    }
}

#[cfg(test)]
mod fixtures;
'''
SWIFT = """
func load() {
  let a = "task.show"
  // "pane.list" in a comment
}
"""


class WireNameTests(unittest.TestCase):
    def fixture(self) -> Path:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        (root / "contracts").mkdir()
        (root / "contracts/protocol.json").write_text(json.dumps(SCHEMA))
        for relative, text in {
            "engine/crate/src/lib.rs": RUST,
            "engine/crate/src/fixtures.rs": 'const A: &str = "pane.list";',
            "engine/crate/tests/api.rs": 'const A: &str = "pane.list";',
            "engine/crate/src/generated.rs": 'pub const A: &str = "pane.list";',
            "apps/shared/Sources/Client/Client.swift": SWIFT,
            "apps/shared/Tests/ClientTests/ClientTests.swift": 'let a = "task.show"',
        }.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        return root

    def test_flags_literals_in_production_code_only(self):
        found = [(path, line, name) for path, line, name, _ in sites(self.fixture())]
        self.assertEqual(
            found,
            [
                ("apps/shared/Sources/Client/Client.swift", 3, "task.show"),
                ("engine/crate/src/lib.rs", 6, "pane.list"),
                ("engine/crate/src/lib.rs", 7, "pane.updated"),
            ],
        )

    def test_reports_whether_a_name_is_a_method_or_an_event(self):
        kinds = {name: kind for _, _, name, kind in sites(self.fixture())}
        self.assertEqual(kinds["pane.updated"], "event")
        self.assertEqual(kinds["pane.list"], "method")


if __name__ == "__main__":
    unittest.main()
