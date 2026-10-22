import copy
import json
import unittest
from pathlib import Path

from generated_header import header_text
from readiness import RUST_OUTPUT, render_readiness, validate

ROOT = Path(__file__).resolve().parent
SCHEMA = json.loads((ROOT / "protocol.json").read_text())
HEADER = f"// {header_text('contracts/protocol.json')}\n"


class ReadinessTests(unittest.TestCase):
    def test_the_table_renders_for_the_engine(self):
        outputs = render_readiness(SCHEMA, HEADER)
        self.assertEqual(set(outputs), {RUST_OUTPUT})
        self.assertIn('"xcode-select --install"', outputs[RUST_OUTPUT])
        self.assertIn('prefix: "brew "', outputs[RUST_OUTPUT])
        self.assertIn('id: "registry.claude-acp"', outputs[RUST_OUTPUT])

    def test_a_hint_belongs_to_a_system_the_entry_runs_on(self):
        changed = copy.deepcopy(SCHEMA["x-readiness"])
        changed["tools"][-1]["hints"]["windows"] = "winget install lsof"
        with self.assertRaises(ValueError):
            validate(changed, SCHEMA)

    def test_an_entry_is_listed_once_and_a_harness_is_a_registry_entry(self):
        changed = copy.deepcopy(SCHEMA["x-readiness"])
        changed["harnesses"].append(copy.deepcopy(changed["harnesses"][0]))
        with self.assertRaises(ValueError):
            validate(changed, SCHEMA)
        changed = copy.deepcopy(SCHEMA["x-readiness"])
        changed["harnesses"][0]["id"] = "local.tool"
        with self.assertRaises(ValueError):
            validate(changed, SCHEMA)

    def test_every_system_the_wire_names_has_a_command_for_each_tool_it_runs_on(self):
        for tool in SCHEMA["x-readiness"]["tools"]:
            if tool["required"]:
                self.assertEqual(set(tool["hints"]), set(tool["systems"]), tool["id"])


if __name__ == "__main__":
    unittest.main()
