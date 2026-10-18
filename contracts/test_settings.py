import copy
import json
import unittest
from pathlib import Path

from generated_header import header_text
from settings import RUST_OUTPUT, SWIFT_OUTPUT, TS_OUTPUT, render_settings, validate

ROOT = Path(__file__).resolve().parent
SCHEMA = json.loads((ROOT / "protocol.json").read_text())
HEADER = f"// {header_text('contracts/protocol.json')}\n"


class SettingsTests(unittest.TestCase):
    def changed(self, edit):
        settings = copy.deepcopy(SCHEMA["x-settings"])
        edit(settings)
        return settings

    def test_every_output_renders_from_the_one_section(self):
        outputs = render_settings(SCHEMA, HEADER)
        self.assertEqual(set(outputs), {RUST_OUTPUT, SWIFT_OUTPUT, TS_OUTPUT})
        self.assertIn('pub const TERMINAL_DEFAULT_COLS: &str = "terminal.defaultCols";', outputs[RUST_OUTPUT])
        self.assertIn('public static let terminalDefaultCols = "terminal.defaultCols"', outputs[SWIFT_OUTPUT])
        self.assertIn('"harness.defaultMode.<harnessId>"', outputs[TS_OUTPUT])

    def test_the_bounds_come_from_the_contract(self):
        settings = self.changed(lambda s: s["terminal.defaultCols"].update(maximum=500))
        outputs = render_settings({"x-settings": settings}, HEADER)
        self.assertIn("(1..=500).contains(&n)", outputs[RUST_OUTPUT])
        self.assertIn("(1...500).contains(number)", outputs[SWIFT_OUTPUT])

    def test_a_default_outside_its_bounds_is_refused(self):
        for edit in (
            lambda s: s["terminal.defaultCols"].update(default=5000),
            lambda s: s["record.default"].update(default="yes"),
            lambda s: s["harness.preferredId"].update(default="x" * 200),
            lambda s: s["terminal.defaultRows"].update(type="number"),
        ):
            with self.assertRaises(ValueError):
                validate(self.changed(edit))

    def test_a_family_needs_its_parameter_and_only_a_family_takes_one(self):
        with self.assertRaises(ValueError):
            validate(self.changed(lambda s: s["harness.defaultMode.<harnessId>"].pop("parameter")))
        with self.assertRaises(ValueError):
            validate(
                self.changed(
                    lambda s: s["harness.preferredId"].update(parameter={"minLength": 1, "maxLength": 9})
                )
            )

    def test_an_undocumented_field_or_a_malformed_key_is_refused(self):
        with self.assertRaises(ValueError):
            validate(self.changed(lambda s: s["record.default"].update(maximum=3)))
        with self.assertRaises(ValueError):
            validate(self.changed(lambda s: s.update({"Record.Default": s["record.default"]})))
        with self.assertRaises(ValueError):
            validate({})

    def test_every_setting_of_the_contract_is_implemented(self):
        # The settings the engine answers to are exactly these: a key the contract lists must
        # not fall through to "Unknown setting" (the web link toggles were once listed and not
        # implemented; the Mac keeps them per device).
        self.assertEqual(
            sorted(SCHEMA["x-settings"]),
            [
                "harness.defaultMode.<harnessId>",
                "harness.preferredId",
                "record.default",
                "terminal.defaultCols",
                "terminal.defaultRows",
            ],
        )


if __name__ == "__main__":
    unittest.main()
