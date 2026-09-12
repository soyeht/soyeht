import copy
import json
import re
import unittest
from pathlib import Path

from generate import lower_camel, render, validate_profiles, validate_schema, variant

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

    def test_event_registry_is_generated_for_every_language(self):
        outputs = render(self.schema, "test-digest")
        rust = outputs["engine/soyeht-protocol/src/generated.rs"]
        swift = outputs["contracts/swift/Sources/SoyehtProtocol/Generated.swift"]
        typescript = outputs["contracts/typescript/src/generated.ts"]
        for name, event in self.schema["x-events"].items():
            params = event["params"]["$ref"].split("/")[-1]
            durable = str(event["durable"]).lower()
            with self.subTest(language="rust", event=name):
                self.assertRegex(
                    rust,
                    rf"name: EventName::{variant(name)},\n\s+"
                    rf'params_type: "{params}",\n\s+durable: {durable}',
                )
            with self.subTest(language="swift", event=name):
                self.assertIn(
                    f'.{lower_camel(name)}: EventDescriptor('
                    f'paramsType: "{params}", durable: {durable})',
                    swift,
                )
            with self.subTest(language="typescript", event=name):
                self.assertIn(
                    f'"{name}": {{ paramsType: "{params}", durable: {durable} }}',
                    typescript,
                )

    def test_event_and_error_registries_fail_closed(self):
        changes = []
        invalid_durable = copy.deepcopy(self.schema)
        invalid_durable["x-events"]["pane.created"]["durable"] = "true"
        changes.append(("durable", invalid_durable))
        missing_event_type = copy.deepcopy(self.schema)
        missing_event_type["x-events"]["pane.created"]["params"] = {
            "$ref": "#/$defs/Missing"
        }
        changes.append(("event type", missing_event_type))
        missing_error_code = copy.deepcopy(self.schema)
        del missing_error_code["x-errorCodes"]["not_found"]
        changes.append(("missing error", missing_error_code))
        duplicate_error_code = copy.deepcopy(self.schema)
        duplicate_error_code["x-errorCodes"]["invalid_params"] = -32000
        changes.append(("duplicate code", duplicate_error_code))
        for label, changed in changes:
            with self.subTest(change=label), self.assertRaises(ValueError):
                validate_schema(changed)

    def test_model_list_source_is_closed(self):
        validate_schema(self.schema)
        changed = copy.deepcopy(self.schema)
        del changed["$defs"]["ModelListSource"]["enum"]
        with self.assertRaises(ValueError):
            validate_schema(changed)

    def test_swift_task_name_does_not_shadow_concurrency(self):
        outputs = render(self.schema, "test-digest")
        swift = outputs["contracts/swift/Sources/SoyehtProtocol/Generated.swift"]
        dispatch = outputs["contracts/swift/Sources/ProtocolVectors/Dispatch.swift"]
        self.assertIn("public struct TaskRecord", swift)
        self.assertNotIn("public struct Task {", swift)
        self.assertIn(
            'case "Task": return try codec(SoyehtProtocol.TaskRecord.self, data)',
            dispatch,
        )


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
