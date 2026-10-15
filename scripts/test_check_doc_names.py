"""Regression tests for the document name check."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from check_doc_names import HISTORICAL_NAMES, ROOT, document_names, problems

SCHEMA = {
    "x-methods": {
        "pane.list": {"params": {"$ref": "#/$defs/ListParams"}, "result": {"$ref": "#/$defs/ListResult"}},
        "agent.message": {"params": {"$ref": "#/$defs/MessageParams"}},
    },
    "x-events": {
        "pane.auth_finished": {"params": {"$ref": "#/$defs/Finished"}},
    },
    "x-settings": {
        "terminal.defaultCols": {},
        "harness.defaultMode.<harnessId>": {},
    },
    "$defs": {
        "ListParams": {"type": "object", "properties": {}},
        "ListResult": {"type": "object", "properties": {"items": {"type": "array"}}},
        "MessageParams": {"type": "object", "properties": {"text": {"type": "string"}}},
        "Finished": {"type": "object", "properties": {"reason": {"type": "string"}}},
        "Pane": {
            "type": "object",
            "properties": {"agent": {"$ref": "#/$defs/Agent"}, "message": {"type": "string"}},
        },
        "Agent": {"type": "object", "properties": {"canSignOut": {"type": "boolean"}}},
    },
}
DOC = """\
Real: `pane.list {paneId}`, `agent.message`, `settings.x` is not in a namespace here.
Field: `agent.canSignOut`, `pane.auth_finished.reason`, `terminal.defaultCols`.
Template: `harness.defaultMode.alpha`.
Log event: `pane.ready`. Files: `pane.sock` and `pane.log.1`.
Gone: `pane.yolo`, `pane.message.list`, `agent.reason`, `pane.message`.

```json
{"method": "pane.list", "other": "pane.queue.steered"}
let value = self.pane.state
```
"""


class DocumentNameTests(unittest.TestCase):
    def fixture(self, extra_docs: dict[str, str] | None = None) -> Path:
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        files = {
            "contracts/protocol.json": json.dumps(SCHEMA),
            "docs/contracts/08.md": DOC,
            "docs/reports/old.md": "`pane.removed` was true then.",
            "engine/crate/src/lib.rs": 'const E: &str = "pane.ready";',
            **(extra_docs or {}),
        }
        for relative, text in files.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        return root

    def names(self, extra_docs: dict[str, str] | None = None) -> list[str]:
        found = problems(self.fixture(extra_docs))
        return [line.split("`")[1] for line in found if line.count("`") >= 2]

    def test_flags_names_the_protocol_and_code_do_not_have(self):
        self.assertEqual(
            self.names(),
            ["pane.yolo", "pane.message.list", "agent.reason", "pane.message", "pane.queue.steered"],
        )

    def test_accepts_methods_fields_settings_log_events_and_files(self):
        flagged = set(self.names())
        for name in (
            "pane.list", "agent.message", "agent.canSignOut", "pane.auth_finished.reason",
            "terminal.defaultCols", "harness.defaultMode.alpha", "pane.ready", "pane.sock",
            "pane.log.1",
        ):  # fmt: skip
            self.assertNotIn(name, flagged)

    def test_dated_reports_are_not_checked(self):
        self.assertNotIn("pane.removed", self.names())

    def test_prose_outside_a_namespace_is_ignored(self):
        self.assertNotIn("settings.x", self.names())
        self.assertNotIn("self.pane.state", self.names())

    def test_a_historical_name_must_still_be_mentioned(self):
        found = problems(self.fixture())
        self.assertTrue(any("HISTORICAL_NAMES lists pane.attention" in line for line in found))
        kept = self.fixture({"docs/map.md": "| `pane.attention {n}` | new |"})
        self.assertFalse(any("pane.attention" in line for line in problems(kept)))

    def test_inline_names_stop_at_arguments(self):
        self.assertEqual(
            [name for _, name in document_names("`pane.list {paneId}` and `a.b`, then `pane.x(1)`")],
            ["pane.list", "a.b", "pane.x"],
        )

    def test_the_repository_documents_only_real_names(self):
        self.assertEqual(problems(ROOT), [])
        self.assertIn("pane.attention", HISTORICAL_NAMES)


if __name__ == "__main__":
    unittest.main()
