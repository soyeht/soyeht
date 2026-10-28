import copy
import json
import unittest
from pathlib import Path

from generated_header import header_text
from client_policy import RUST_OUTPUT, SWIFT_OUTPUT, TS_OUTPUT, render_client_policy, validate

ROOT = Path(__file__).resolve().parent
SCHEMA = json.loads((ROOT / "protocol.json").read_text())
HEADER = f"// {header_text('contracts/protocol.json')}\n"


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

    def test_a_debug_build_name_is_the_release_name_and_dev(self):
        names = SCHEMA["x-clientPolicy"]["clientName"]
        self.assertEqual(names["macDev"], "mac-dev")
        for bad in ({"macDev": "mac-debug"}, {"mac": "iphone", "macDev": "iphone-dev"}):
            policy = copy.deepcopy(SCHEMA["x-clientPolicy"])
            policy["clientName"].update(bad)
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                validate(policy, SCHEMA)
        policy = copy.deepcopy(SCHEMA["x-clientPolicy"])
        del policy["clientName"]["desktopDev"]
        with self.assertRaises(ValueError):
            validate(policy, SCHEMA)

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

    def test_a_page_size_stays_within_the_limit_of_its_request(self):
        for value in (0, 65, "most", 1.5):
            changed = copy.deepcopy(SCHEMA["x-clientPolicy"])
            changed["pageSize"]["queue"] = value
            with self.assertRaises(ValueError):
                validate(changed, SCHEMA)

    def test_the_maximum_is_the_requests_own(self):
        outputs = render_client_policy(SCHEMA, HEADER)
        self.assertIn("pub const PAGE_SIZE_QUEUE: u32 = 64;", outputs[RUST_OUTPUT])
        self.assertIn("static let pageSizeModels: UInt32 = 64", outputs[SWIFT_OUTPUT])
        self.assertNotIn('"maximum"', outputs[TS_OUTPUT])

    def test_every_pane_state_has_a_dot_and_a_running_flag(self):
        missing = copy.deepcopy(SCHEMA["x-clientPolicy"])
        del missing["paneStates"]["dead"]
        with self.assertRaises(ValueError):
            validate(missing, SCHEMA)
        unknown_dot = copy.deepcopy(SCHEMA["x-clientPolicy"])
        unknown_dot["paneStates"]["dead"]["dot"] = "magenta"
        with self.assertRaises(ValueError):
            validate(unknown_dot, SCHEMA)
        outputs = render_client_policy(SCHEMA, HEADER)
        self.assertIn("pub fn is_turn_running(&self) -> bool", outputs[RUST_OUTPUT])
        self.assertIn("public var isTurnRunning: Bool", outputs[SWIFT_OUTPUT])

    def test_a_cap_is_a_positive_whole_number_and_is_a_usize_in_rust(self):
        for value in (0, -1, 1.5, "128"):
            changed = copy.deepcopy(SCHEMA["x-clientPolicy"])
            changed["caps"]["webLinks"] = value
            with self.assertRaises(ValueError):
                validate(changed, SCHEMA)
        outputs = render_client_policy(SCHEMA, HEADER)
        self.assertIn("pub const CAPS_WEB_LINKS: usize = 128;", outputs[RUST_OUTPUT])
        self.assertIn("static let capsModelPages = 32", outputs[SWIFT_OUTPUT])

    def test_the_inline_image_types_are_distinct_media_types(self):
        for types in ([], ["image/png", "image/png"], ["png"]):
            changed = copy.deepcopy(SCHEMA["x-clientPolicy"])
            changed["inlineImageTypes"] = types
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
