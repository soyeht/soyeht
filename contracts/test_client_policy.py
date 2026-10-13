import copy
import json
import unittest
from pathlib import Path

from client_policy import RUST_OUTPUT, SWIFT_OUTPUT, TS_OUTPUT, render_client_policy, validate

ROOT = Path(__file__).resolve().parent
SCHEMA = json.loads((ROOT / "protocol.json").read_text())
HEADER = "// Generated from contracts/protocol.json (sha256 test). Do not edit.\n"


class ClientPolicyTests(unittest.TestCase):
    def test_every_output_renders_from_the_one_section(self):
        outputs = render_client_policy(SCHEMA, HEADER)
        self.assertEqual(set(outputs), {RUST_OUTPUT, SWIFT_OUTPUT, TS_OUTPUT})
        policy = SCHEMA["x-clientPolicy"]
        self.assertIn(
            f"pub const REQUEST_TIMEOUT_MS: u64 = {policy['timeouts']['requestTimeoutMs']};",
            outputs[RUST_OUTPUT],
        )
        self.assertIn(
            f"static let requestTimeoutMs = {policy['timeouts']['requestTimeoutMs']}",
            outputs[SWIFT_OUTPUT],
        )
        self.assertIn('"requestTimeoutMs"', outputs[TS_OUTPUT])

    def test_a_timeout_is_whole_positive_milliseconds(self):
        for value in (0, -5, 1.5, "12"):
            changed = copy.deepcopy(SCHEMA["x-clientPolicy"])
            changed["timeouts"]["requestTimeoutMs"] = value
            with self.assertRaises(ValueError):
                validate(changed, SCHEMA)

    def test_every_usage_provider_has_exactly_one_name(self):
        changed = copy.deepcopy(SCHEMA["x-clientPolicy"])
        del changed["usageProviderNames"]["cursor"]
        with self.assertRaises(ValueError):
            validate(changed, SCHEMA)

    def test_the_roles_are_closed(self):
        extra = copy.deepcopy(SCHEMA["x-clientPolicy"])
        extra["configRoles"]["verbosity"] = {"category": "verbosity"}
        with self.assertRaises(ValueError):
            validate(extra, SCHEMA)
        unknown_field = copy.deepcopy(SCHEMA["x-clientPolicy"])
        unknown_field["configRoles"]["model"]["alias"] = "m"
        with self.assertRaises(ValueError):
            validate(unknown_field, SCHEMA)


if __name__ == "__main__":
    unittest.main()
