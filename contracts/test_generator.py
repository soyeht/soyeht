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

    def test_pane_route_registry_follows_every_declared_parameter(self):
        rust = render(self.schema, "test-digest")["engine/soyeht-protocol/src/generated.rs"]
        registry = rust.split("pub const PANE_ROUTE_METHODS", 1)[1].split("];", 1)[0]
        actual = dict(re.findall(r'\("([^"]+)", "([^"]+)"\)', registry))
        expected = {}
        for name, method in self.schema["x-methods"].items():
            definition = self.schema["$defs"][method["params"]["$ref"].split("/")[-1]]
            properties = definition.get("properties", {})
            identifier_names = {
                field
                for field, rule in properties.items()
                if rule == {"$ref": "#/$defs/Id"}
                or (
                    rule.get("type") == "array"
                    and rule.get("items") == {"$ref": "#/$defs/Id"}
                )
            }
            route = method.get("x-paneRoute")
            if route is None and "paneId" in properties:
                route = "paneId"
            classified = identifier_names & set(self.schema["x-paneNonRouteIds"])
            if route is not None:
                classified.add(route)
                expected[name] = route
            with self.subTest(method=name):
                self.assertEqual(classified, identifier_names)
        self.assertEqual(actual, expected)

    def test_unclassified_pane_parameter_fails_generation(self):
        additions = copy.deepcopy(self.schema)
        method = additions["x-methods"]["usage.refresh"]
        definition = additions["$defs"][method["params"]["$ref"].split("/")[-1]]
        definition["properties"]["relatedPane"] = {"$ref": "#/$defs/Id"}

        missing_override = copy.deepcopy(self.schema)
        del missing_override["x-methods"]["writing.variant.request"]["x-paneRoute"]

        missing_non_route = copy.deepcopy(self.schema)
        missing_non_route["x-paneNonRouteIds"].remove("paneIds")

        stale_non_route = copy.deepcopy(self.schema)
        stale_non_route["x-paneNonRouteIds"].append("unusedId")

        for label, changed in (
            ("usage.refresh", additions),
            ("writing.variant.request", missing_override),
            ("pane.experiment.export", missing_non_route),
            ("unused", stale_non_route),
        ):
            with self.subTest(method=label):
                with self.assertRaisesRegex(ValueError, "decision|unused"):
                    validate_schema(changed)

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

    def test_editor_surface_is_complete_bounded_and_cursor_safe(self):
        expected_methods = {
            "file.open", "file.read", "file.write", "file.watch", "file.history",
            "file.revert", "file.undo", "file.redo", "file.search", "file.symbols",
            "file.definition", "file.references", "file.diagnostics", "file.diff",
            "file.format",
            "editor.review.accept", "editor.review.reject", "editor.review.comment",
            "lsp.status", "editor.tabs.list", "editor.tabs.move",
        }
        expected_events = {
            "file.changed_on_disk", "file.saved", "file.conflict",
            "lsp.diagnostics", "editor.agent_cursor", "editor.review_requested",
        }
        self.assertLessEqual(expected_methods, set(self.schema["x-methods"]))
        self.assertLessEqual(expected_events, set(self.schema["x-events"]))
        self.assertIn("editor", self.schema["$defs"]["PaneMode"]["enum"])
        self.assertIn("files", self.schema["$defs"]["SnapshotSection"]["enum"])

        read = self.schema["$defs"]["FileReadParams"]
        self.assertEqual(
            read["properties"]["lineCount"]["maximum"],
            self.schema["x-limits"]["editorViewportLines"],
        )
        self.assertNotIn("content", self.schema["$defs"]["FileOpenResult"]["properties"])
        self.assertNotIn("content", self.schema["$defs"]["FileConflictEvent"]["properties"])
        for name in expected_events:
            self.assertTrue(self.schema["x-events"][name]["durable"], name)

    def test_every_editor_mutation_has_revision_or_is_an_open_or_watch_operation(self):
        exempt = {"file.open", "file.watch", "file.search", "editor.review.comment"}
        for name, method in self.schema["x-methods"].items():
            if not method["mutation"] or not (
                name.startswith("file.") or name.startswith("editor.review.")
            ):
                continue
            definition = self.schema["$defs"][method["params"]["$ref"].split("/")[-1]]
            self.assertIn("idempotencyKey", definition["required"], name)
            if name not in exempt:
                self.assertIn("baseRevision", definition["required"], name)

    def test_project_panel_surface_is_path_based_and_bounded(self):
        expected_methods = {
            "project.tree.list", "project.tree.watch", "project.entry.create",
            "project.entry.rename", "project.entry.delete",
        }
        self.assertLessEqual(expected_methods, set(self.schema["x-methods"]))
        self.assertFalse(self.schema["x-events"]["project.tree_changed"]["durable"])
        self.assertTrue(self.schema["x-events"]["project.entry_renamed"]["durable"])
        self.assertTrue(self.schema["x-events"]["editor.tabs_updated"]["durable"])
        for name in expected_methods:
            method = self.schema["x-methods"][name]
            definition = self.schema["$defs"][method["params"]["$ref"].split("/")[-1]]
            self.assertIn("workspaceId", definition["required"], name)
            if method["mutation"]:
                self.assertIn("idempotencyKey", definition["required"], name)
        self.assertIn("preview", self.schema["$defs"]["FileOpenParams"]["properties"])
        limits = self.schema["x-limits"]
        self.assertEqual(limits["projectTreeEntries"], 5000)
        self.assertEqual(limits["projectTreeWatches"], 256)

    def test_browser_surface_is_complete_additive_and_bounded(self):
        expected_methods = {
            "servers.local.list",
            "web.open", "web.navigate", "web.snapshot", "web.click", "web.type",
            "web.press", "web.hover", "web.select", "web.scroll", "web.wait",
            "web.evaluate", "web.screenshot", "web.console", "web.network",
            "web.resize", "web.close", "web.command", "web.host.navigated",
            "web.host.console_counts",
        }
        expected_events = {
            "servers.local.changed", "web.navigated", "web.console_counts",
            "web.agent_driving",
        }
        expected_errors = {
            "stale_reference", "human_took_over", "web_host_unavailable",
            "navigation_refused",
        }
        self.assertLessEqual(expected_methods, set(self.schema["x-methods"]))
        self.assertLessEqual(expected_events, set(self.schema["x-events"]))
        self.assertLessEqual(expected_errors, set(self.schema["x-errorCodes"]))
        self.assertIn("web", self.schema["$defs"]["PaneMode"]["enum"])
        self.assertEqual(
            self.schema["$defs"]["HelloParams"]["properties"]["capabilities"],
            {"type": "array", "items": {"$ref": "#/$defs/ClientCapability"}},
        )
        self.assertNotIn(
            "capabilities", self.schema["$defs"]["HelloParams"]["required"]
        )

        limits = self.schema["x-limits"]
        self.assertEqual(limits["webPanesPerWorkspace"], 10)
        self.assertEqual(limits["webSnapshotBytes"], 200 * 1024)
        self.assertEqual(limits["webScreenshotBytes"], 2 * 1024 * 1024)
        self.assertEqual(limits["webEvaluateBytes"], 64 * 1024)
        self.assertEqual(limits["webConsoleEntries"], 500)
        self.assertEqual(limits["webNetworkEntries"], 300)
        self.assertEqual(limits["webWaitTimeoutMs"], 60_000)
        self.assertEqual(limits["localServerEntries"], 50)
        self.assertEqual(limits["webPickedElements"], 20)

        for name in ("WebConsoleEntry", "WebNetworkEntry"):
            properties = set(self.schema["$defs"][name]["properties"])
            self.assertNotIn("headers", properties)
            self.assertNotIn("body", properties)
        self.assertEqual(
            self.schema["$defs"]["WebPickedElement"]["properties"]["outerHtml"][
                "maxLength"
            ],
            limits["webOuterHtmlBytes"],
        )
        self.assertFalse(self.schema["x-events"]["servers.local.changed"]["durable"])
        self.assertFalse(self.schema["x-events"]["web.console_counts"]["durable"])
        self.assertFalse(self.schema["x-events"]["web.agent_driving"]["durable"])
        for name in (
            "ServersLocalChangedEvent",
            "WebConsoleCountsEvent",
            "WebAgentDrivingEvent",
        ):
            required = set(self.schema["$defs"][name]["required"])
            properties = set(self.schema["$defs"][name]["properties"])
            self.assertNotIn("eventId", required, name)
            self.assertNotIn("at", required, name)
            self.assertNotIn("eventId", properties, name)
            self.assertNotIn("at", properties, name)

    def test_browser_mutations_require_idempotency_but_reverse_commands_use_operation_keys(self):
        for name, method in self.schema["x-methods"].items():
            if not name.startswith("web.") or not method["mutation"]:
                continue
            definition = self.schema["$defs"][method["params"]["$ref"].split("/")[-1]]
            self.assertIn("idempotencyKey", definition["required"], name)
        command = self.schema["$defs"]["WebCommandParams"]
        self.assertIn("operationKey", command["required"])
        self.assertNotIn("idempotencyKey", command["properties"])
        self.assertFalse(self.schema["x-methods"]["web.command"]["mutation"])

    def test_web_session_permission_is_explicit_and_independent_of_agent_modes(self):
        kinds = self.schema["$defs"]["PermissionRequestKind"]["enum"]
        self.assertEqual(kinds, ["agent", "web_session", "web_evaluate"])
        for name in ("PermissionRequest", "PanePermissionRequestedEvent"):
            properties = self.schema["$defs"][name]["properties"]
            self.assertEqual(properties["kind"], {"$ref": "#/$defs/PermissionRequestKind"})
            self.assertIn("domain", properties)

    def test_harness_switch_surface_is_additive_durable_and_bounded(self):
        self.assertEqual(self.schema["x-protocolVersion"], 1)
        method = self.schema["x-methods"]["pane.harness.switch"]
        self.assertTrue(method["mutation"])
        params_name = method["params"]["$ref"].split("/")[-1]
        self.assertIn(
            "idempotencyKey", self.schema["$defs"][params_name]["required"]
        )
        for name in (
            "pane.harness_switch_started",
            "pane.harness_switch_done",
            "pane.harness_switch_failed",
        ):
            self.assertTrue(self.schema["x-events"][name]["durable"], name)
        self.assertEqual(self.schema["x-errorCodes"]["harness_switch_failed"], -32035)
        self.assertEqual(self.schema["x-limits"]["harnessContinuationBytes"], 15_999)
        self.assertEqual(self.schema["x-limits"]["harnessContinuationMessages"], 6)

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
