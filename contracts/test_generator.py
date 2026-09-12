import copy
import json
import re
import unittest
from pathlib import Path

from generate import validate_profiles, validate_schema

ROOT = Path(__file__).resolve().parent


class SchemaTests(unittest.TestCase):
    def setUp(self):
        self.schema = json.loads((ROOT / "protocol.json").read_text())

    def test_unknown_keywords_and_references_fail_closed(self):
        validate_schema(self.schema)
        for patch in ({"unknownKeyword": True}, {"$ref": "#/$defs/Missing"}):
            changed = copy.deepcopy(self.schema)
            changed["$defs"]["Id"].update(patch)
            with self.assertRaises(ValueError):
                validate_schema(changed)

    def test_every_method_and_event_has_a_wire_vector(self):
        names = {v["name"] for v in json.loads((ROOT / "vectors/messages.json").read_text())}
        for name in self.schema["x-methods"]:
            self.assertIn("request:" + name, names)
            self.assertIn("response:" + name, names)
        for name in self.schema["x-events"]:
            self.assertIn("event:" + name, names)

    def test_mutations_require_an_idempotency_key(self):
        for name, method in self.schema["x-methods"].items():
            if method["mutation"]:
                definition = self.schema["$defs"][method["params"]["$ref"].split("/")[-1]]
                self.assertIn("idempotencyKey", definition["required"], name)


class ProfileTests(unittest.TestCase):
    def setUp(self):
        self.document = json.loads((ROOT / "profiles.json").read_text())

    def test_dev_profile_is_complete_and_safe_to_generate(self):
        validate_profiles(self.document)
        changes = (
            ("dataDirectoryFromHome", "/tmp/dev"),
            ("dataDirectoryFromHome", "Library/Application Support/../dev"),
            ("tcpPort", 0),
            ("tcpPort", True),
        )
        for field, value in changes:
            changed = copy.deepcopy(self.document)
            changed["profiles"]["dev"][field] = value
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                validate_profiles(changed)

    def test_profile_components_cannot_drift_or_share_endpoints(self):
        for field in ("sockets", "launchdLabels"):
            changed = copy.deepcopy(self.document)
            changed["profiles"]["dev"][field]["ptyd"] = changed["profiles"]["dev"][field][
                "engine"
            ]
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate_profiles(changed)

    def test_profile_values_are_not_typed_again_in_runtime_sources(self):
        profile = self.document["profiles"]["dev"]
        values = {
            profile["dataDirectoryFromHome"],
            str(profile["tcpPort"]),
            *profile["sockets"].values(),
            *profile["launchdLabels"].values(),
        }
        paths = [
            *ROOT.parent.glob("engine/*/src/**/*.rs"),
            *ROOT.parent.glob("apps/*/Sources/**/*.swift"),
            *ROOT.parent.glob("scripts/*.py"),
        ]
        generated = {
            ROOT.parent / "engine/soyeht-protocol/src/profile.rs",
            ROOT.parent / "scripts/generated_profile.py",
        }
        for path in paths:
            if path in generated:
                continue
            source = path.read_text()
            for value in values:
                pattern = rf"(?<![A-Za-z0-9]){re.escape(value)}(?![A-Za-z0-9])"
                with self.subTest(path=path.relative_to(ROOT.parent), value=value):
                    self.assertIsNone(re.search(pattern, source))


if __name__ == "__main__":
    unittest.main()
