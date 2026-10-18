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
    "x-settings": {
        "terminal.defaultCols": {},
        "harness.defaultMode.<harnessId>": {},
    },
    "x-errorCodes": {"not_found": -32000, "stale_version": -32007},
}
RUST = '''
use soyeht_protocol::method;

fn production() {
    let ok = method::PANE_LIST;
    let typed = "pane.list";
    let event = json!({"method": "pane.updated"});
    let plain = "hello";
    let other = "pane.listing";
    let setting = "terminal.defaultCols";
    let family = format!("harness.defaultMode.{id}");
    let code = "not_found";
    let near = "not_found_yet";
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
  let b = failure ?? "stale_version"
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
            "engine/crate/src/setting.rs": (
                '// Generated from contracts/protocol.json. Do not edit.\n'
                'pub const A: &str = "terminal.defaultCols";'
            ),
            "engine/soyeht-ptyd/src/wire_kind.rs": 'pub const A: &str = "not_found";\nconst B: &str = "pane.list";',
            "engine/soyeht-engine/src/outbox.rs": 'const A: &str = "not_found";',
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
                ("apps/shared/Sources/Client/Client.swift", 4, "stale_version"),
                ("engine/crate/src/lib.rs", 6, "pane.list"),
                ("engine/crate/src/lib.rs", 7, "pane.updated"),
                ("engine/crate/src/lib.rs", 10, "terminal.defaultCols"),
                ("engine/crate/src/lib.rs", 11, "harness.defaultMode.{id}"),
                ("engine/crate/src/lib.rs", 12, "not_found"),
                ("engine/soyeht-ptyd/src/wire_kind.rs", 2, "pane.list"),
            ],
        )

    def test_reports_whether_a_name_is_a_method_an_event_or_a_setting(self):
        kinds = {name: kind for _, _, name, kind in sites(self.fixture())}
        self.assertEqual(kinds["pane.updated"], "event")
        self.assertEqual(kinds["pane.list"], "method")
        self.assertEqual(kinds["terminal.defaultCols"], "setting")
        self.assertEqual(kinds["harness.defaultMode.{id}"], "setting")
        self.assertEqual(kinds["not_found"], "error code")


if __name__ == "__main__":
    unittest.main()
