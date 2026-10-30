import copy
import json
import re
import unittest
from pathlib import Path

from design_tokens import render_tokens
from generate import (
    lower_camel,
    render,
    render_profiles,
    snake,
    validate_profiles,
    validate_schema,
    variant,
)
from generated_header import PREFIX, header_text
from legacy import render_legacy, validate_legacy

ROOT = Path(__file__).resolve().parent


class GeneratedHeaderTests(unittest.TestCase):
    """A header that changes with the source makes every output conflict between two pull requests."""

    def outputs(self):
        schema = json.loads((ROOT / "protocol.json").read_text())
        profiles = json.loads((ROOT / "profiles.json").read_text())
        tokens = json.loads((ROOT / "design-tokens.json").read_text())
        legacy = json.loads((ROOT / "legacy.json").read_text())
        return {
            **render(schema),
            **render_profiles(profiles),
            **render_tokens(tokens),
            **render_legacy(legacy),
        }

    def test_header_names_the_source_and_carries_no_hash(self):
        generated = 0
        for path, text in self.outputs().items():
            header = next((line for line in text.splitlines()[:2] if "Generated from" in line), None)
            if header is None:
                continue  # schema.json is a copy of the source and has no header
            generated += 1
            self.assertRegex(header, rf"^(// |# |<!-- )?{PREFIX}[a-z-]+\.json\. Do not edit\.( -->)?$", path)
            self.assertNotRegex(header, r"[0-9a-f]{16}|sha256", path)
        self.assertGreaterEqual(generated, 15)

    def test_header_is_a_fixed_line_per_source(self):
        self.assertEqual(
            header_text("contracts/protocol.json"),
            "Generated from contracts/protocol.json. Do not edit.",
        )
        with self.assertRaises(ValueError):
            header_text("elsewhere/protocol.json")

    def test_every_output_is_marked_generated_for_reviewers(self):
        attributes = (ROOT.parent / ".gitattributes").read_text().splitlines()
        marked = {line.split()[0] for line in attributes if line.endswith("linguist-generated=true")}
        self.assertEqual(marked, set(self.outputs()))

    def test_output_does_not_depend_on_unrelated_source_edits(self):
        schema = json.loads((ROOT / "protocol.json").read_text())
        changed = copy.deepcopy(schema)
        changed["x-limits"]["jsonBytes"] += 1  # any edit to the source
        before = render(schema)["engine/soyeht-protocol/src/limits.rs"].splitlines()[0]
        after = render(changed)["engine/soyeht-protocol/src/limits.rs"].splitlines()[0]
        self.assertEqual(before, after)


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

    def test_contract_checks_name_every_schema_and_every_event_has_typed_parameters(self):
        outputs = render(self.schema)
        checks = outputs["engine/soyeht-protocol/src/contract/roundtrip.rs"]
        for name in self.schema["$defs"]:
            self.assertRegex(checks, rf'"{name}" =>\s*\{{?\s*decode_encode::<\s*{name},?\s*>\(')
        rust = outputs["engine/soyeht-protocol/src/generated.rs"]
        uses = {}
        for name, event in self.schema["x-events"].items():
            uses.setdefault(event["params"]["$ref"].split("/")[-1], []).append(name)
        for type_, names in uses.items():
            if len(names) != 1 or self.schema["$defs"][type_].get("type") != "object":
                continue
            durable = self.schema["x-events"][names[0]]["durable"]
            target = type_.removesuffix("Event") + "Draft" if durable else type_
            self.assertIn(f"impl EventParams for {target} {{", rust)
            if durable:
                # The store assigns these; the engine never types them.
                draft = rust.split(f"pub struct {target} {{", 1)[1].split("\n}", 1)[0]
                for key in ("eventId", "at"):
                    self.assertNotIn(f'rename = "{key}"', draft)

    def test_every_durable_event_may_carry_the_actor_the_store_adds(self):
        for name, event in self.schema["x-events"].items():
            if event["durable"]:
                definition = self.schema["$defs"][event["params"]["$ref"].split("/")[-1]]
                if definition.get("type") == "object":
                    self.assertIn("actor", definition["properties"], name)

    def test_mutations_require_an_idempotency_key(self):
        for name, method in self.schema["x-methods"].items():
            if method["mutation"]:
                definition = self.schema["$defs"][method["params"]["$ref"].split("/")[-1]]
                self.assertIn("idempotencyKey", definition["required"], name)

    def test_pane_route_registry_follows_every_declared_parameter(self):
        rust = render(self.schema)["engine/soyeht-protocol/src/generated.rs"]
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
        outputs = render(self.schema)
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

    def test_every_method_and_event_name_has_a_generated_form(self):
        outputs = render(self.schema)
        rust = outputs["engine/soyeht-protocol/src/generated.rs"]
        swift = outputs["contracts/swift/Sources/SoyehtProtocol/Generated.swift"]
        for kind, names in (("method", self.schema["x-methods"]), ("event", self.schema["x-events"])):
            for name in names:
                with self.subTest(kind=kind, name=name):
                    self.assertIn(f'pub const {snake(variant(name)).upper()}: &str = "{name}";', rust)
                    self.assertIn(f'case {lower_camel(name)} = "{name}"', swift)

    def test_frame_kinds_are_generated_for_both_languages(self):
        outputs = render(self.schema)
        rust = outputs["engine/soyeht-protocol/src/limits.rs"]
        swift = outputs["contracts/swift/Sources/SoyehtProtocol/Limits.swift"]
        for name, value in self.schema["x-framing"]["types"].items():
            self.assertIn(f"pub const FRAME_KIND_{name.upper()}: u8 = {value};", rust)
            self.assertIn(f"public static let {name}: UInt8 = {value}", swift)

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
            "file.revert", "file.undo", "file.redo", "file.search", "file.find",
            "file.replace", "file.symbols",
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
            "web.evaluate", "web.screenshot", "web.read", "web.console", "web.network",
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
        self.assertEqual(limits["webAgentImageBytes"], 256 * 1024)
        # The picture, as base64 and carried twice, and its text fit one harness message.
        self.assertLess(limits["webAgentImageBytes"] * 4 // 3 * 2, limits["acpMessageBytes"])
        self.assertEqual(limits["webEvaluateBytes"], 64 * 1024)
        self.assertEqual(limits["webConsoleEntries"], 500)
        self.assertEqual(limits["webNetworkEntries"], 300)
        self.assertEqual(limits["webWaitTimeoutMs"], 60_000)
        self.assertEqual(limits["localServerEntries"], 50)
        self.assertEqual(limits["webPickedElements"], 20)
        self.assertEqual(limits["webReadScreens"], 12)
        self.assertLessEqual(limits["webReadScreensDefault"], limits["webReadScreens"])
        self.assertEqual(limits["webReadTextBytes"], 64 * 1024)
        self.assertEqual(
            self.schema["$defs"]["WebReadParams"]["properties"]["screens"]["maximum"],
            limits["webReadScreens"],
        )
        self.assertEqual(
            self.schema["$defs"]["WebReadResult"]["properties"]["text"]["maxLength"],
            limits["webReadTextBytes"],
        )

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

    def test_web_field_bounds_are_the_named_limits(self):
        """Each web string or extent is bounded by the limit that names it."""
        limits = self.schema["x-limits"]
        bounds = {
            "webFieldBytes": [
                ("WebSnapshotNode", "value"),
                ("WebConsoleEntry", "source"),
                ("WebNetworkEntry", "url"),
                ("WebComponentSource", "sourceFile"),
                ("WebPickedElement", "selector"),
                ("WebElementPackage", "url"),
                ("WebOpenParams", "url"),
                ("WebNavigateParams", "url"),
                ("WebWaitParams", "text"),
                ("WebWaitParams", "selector"),
                ("WebSelectParams", "value"),
            ],
            "webTextBytes": [("WebConsoleEntry", "text")],
            "webLabelBytes": [
                ("WebSnapshotNode", "name"),
                ("WebSnapshotNode", "state"),
                ("WebAccessibility", "name"),
                ("WebAccessibility", "state"),
            ],
            "webTitleBytes": [("WebElementPackage", "title")],
            "webPartBytes": [
                ("WebComputedStyles", "background"),
                ("WebComputedStyles", "font"),
                ("WebComputedStyles", "border"),
                ("WebComponentSource", "component"),
            ],
            "webTokenBytes": [
                ("WebSnapshotNode", "role"),
                ("WebAccessibility", "role"),
                ("WebComputedStyles", "boxSizing"),
                ("WebComputedStyles", "color"),
                ("WebComputedStyles", "borderRadius"),
                ("WebComputedStyles", "display"),
                ("WebComputedStyles", "position"),
            ],
        }
        definitions = self.schema["$defs"]
        for limit, fields in bounds.items():
            for definition, field in fields:
                self.assertEqual(
                    definitions[definition]["properties"][field]["maxLength"],
                    limits[limit],
                    f"{definition}.{field} against {limit}",
                )
        self.assertEqual(
            definitions["WebPickedElement"]["properties"]["path"]["items"]["maxLength"],
            limits["webPartBytes"],
        )
        for definition in ("WebViewport", "WebRect"):
            for field in ("width", "height"):
                self.assertEqual(
                    definitions[definition]["properties"][field]["maximum"],
                    limits["webExtentMax"],
                    f"{definition}.{field}",
                )

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
        outputs = render(self.schema)
        swift = outputs["contracts/swift/Sources/SoyehtProtocol/Generated.swift"]
        dispatch = outputs["contracts/swift/Sources/ProtocolVectors/Dispatch.swift"]
        self.assertIn("public struct TaskRecord", swift)
        self.assertNotIn("public struct Task {", swift)
        self.assertIn(
            'case "Task": return try codec(SoyehtProtocol.TaskRecord.self, data)',
            dispatch,
        )


class LegacyListTests(unittest.TestCase):
    def test_the_list_is_validated_and_generated_for_both_guards(self):
        document = json.loads((ROOT / "legacy.json").read_text())
        validate_legacy(document)
        outputs = render_legacy(document)
        self.assertEqual(
            set(outputs), {"engine/soyeht-protocol/src/legacy.rs", "scripts/generated_legacy.py"}
        )
        for broken in (
            {**document, "applications": ["Soyeht"]},
            {**document, "dataFolders": ["a/b"]},
            {**document, "bundleIds": []},
            {**document, "version": 2},
        ):
            with self.assertRaises(ValueError):
                validate_legacy(broken)


class ProfileTests(unittest.TestCase):
    def setUp(self):
        self.document = json.loads((ROOT / "profiles.json").read_text())

    def changed(self, *path_and_value):
        *path, value = path_and_value
        document = copy.deepcopy(self.document)
        target = document
        for key in path[:-1]:
            target = target[key]
        target[path[-1]] = value
        return document

    def test_profiles_are_complete_and_safe_to_generate(self):
        validate_profiles(self.document)
        changes = (
            ("profiles", "dev", "dataDirectory", "macos", "/tmp/dev"),
            ("profiles", "dev", "dataDirectory", "macos", "Soyeht.next/../dev"),
            ("profiles", "dev", "dataDirectory", "windows", "Soyeht.next\\dev"),
            ("profiles", "dev", "dataDirectory", "linux", "soyeht-next/"),
            ("profiles", "dev", "tcpPort", 0),
            ("profiles", "dev", "displayNames", {"macApp": "Soyeht Dev"}),
            ("profiles", "dev", "displayNames", "window", "  "),
            ("profiles", "dev", "displayNames", "windowsFolder", "Soyeht Dev"),
            ("profiles", "dev", "displayNames", "windowsFolder", "Soyeht\\Next"),
            ("profiles", "dev", "displayNames", "windowsFolder", "Soyeht Next."),
            ("profiles", "dev", "displayNames", "windowsFolder", "Soyeht: Next"),
            # Names Windows keeps for devices, in any case, with or without an extension.
            *(("profiles", "dev", "displayNames", "windowsFolder", device)
              for device in ("CON", "con", "Nul", "AUX.next", "prn.d", "COM1", "lpt9", "Com3.txt", "COM\u00b2", "CONOUT$")),
            ("profiles", "prod", "displayNames", "windowsFolder", self.document["profiles"]["dev"]["displayNames"]["windowsFolder"]),
            ("profiles", "prod", "displayNames", dict(self.document["profiles"]["dev"]["displayNames"])),
            ("profiles", "dev", "tcpPort", True),
            ("profiles", "prod", "tcpPort", self.document["profiles"]["dev"]["tcpPort"]),
            ("profiles", "prod", "appIds", "linux", "Not A Name"),
            ("profiles", "prod", "dataDirectory", {"macos": "Soyeht.next"}),
            ("selection", "environment", "PROFILE"),
            ("version", 1),
        )
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                validate_profiles(self.changed(*change))

    def test_service_names_must_be_safe_distinct_and_for_their_component(self):
        changes = (
            ("profiles", "dev", "systemdUnits", {"engine": "soyeht-next-dev-engine.service"}),
            ("profiles", "dev", "systemdUnits", {
                "engine": "soyeht-next-dev-ptyd.service", "ptyd": "soyeht-next-dev-ptyd.service"}),
            ("profiles", "dev", "systemdUnits", {
                "engine": "soyeht-next-dev-engine", "ptyd": "soyeht-next-dev-ptyd.service"}),
            ("profiles", "dev", "systemdUnits", {
                "engine": "soyeht next-engine.service", "ptyd": "soyeht-next-dev-ptyd.service"}),
            ("profiles", "dev", "windowsTasks", {
                "engine": "Soyeht.Next.Dev.Ptyd", "ptyd": "Soyeht.Next.Dev.Ptyd"}),
            ("profiles", "dev", "windowsTasks", {
                "engine": "Soyeht Next Dev Engine", "ptyd": "Soyeht.Next.Dev.Ptyd"}),
            ("profiles", "dev", "windowsTasks", {
                "engine": "Soyeht.Next.Dev.Engine'; calc", "ptyd": "Soyeht.Next.Dev.Ptyd"}),
            ("profiles", "prod", "windowsTasks", self.document["profiles"]["dev"]["windowsTasks"]),
            ("profiles", "prod", "systemdUnits", self.document["profiles"]["dev"]["systemdUnits"]),
        )
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                validate_profiles(self.changed(*change))

    def test_profile_components_cannot_drift_or_share_endpoints(self):
        for path in (("sockets",), ("profiles", "dev", "launchdLabels")):
            document = copy.deepcopy(self.document)
            values = document
            for key in path:
                values = values[key]
            values["ptyd"] = values["engine"]
            with self.subTest(path=path), self.assertRaises(ValueError):
                validate_profiles(document)

    def test_log_file_names_are_distinct_log_basenames(self):
        self.assertEqual(set(self.document["logs"]), {"engine", "ptyd", "desktop"})
        changes = (
            ("logs", "ptyd", self.document["logs"]["engine"]),
            ("logs", "desktop", "logs/desktop.log"),
            ("logs", "engine", "engine.txt"),
        )
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                validate_profiles(self.changed(*change))
        document = copy.deepcopy(self.document)
        del document["logs"]["desktop"]
        with self.assertRaises(ValueError):
            validate_profiles(document)

    def test_every_identifier_carries_next_so_none_can_name_the_legacy_app(self):
        # The legacy app uses com.soyeht.mac and the data folders Soyeht and Soyeht Dev.
        changes = (
            ("profiles", "prod", "dataDirectory", "macos", "Soyeht"),
            ("profiles", "prod", "dataDirectory", "linux", "soyeht"),
            ("profiles", "prod", "appIds", "macos", "com.soyeht.mac"),
            ("profiles", "dev", "appIds", "macos", "com.soyeht.mac.dev"),
            ("profiles", "prod", "launchdLabels", {
                "engine": "com.soyeht.engine", "ptyd": "com.soyeht.ptyd"}),
            ("profiles", "prod", "systemdUnits", {
                "engine": "soyeht-engine.service", "ptyd": "soyeht-ptyd.service"}),
            ("profiles", "prod", "windowsTasks", {
                "engine": "Soyeht.Engine", "ptyd": "Soyeht.Ptyd"}),
        )
        for change in changes:
            with self.subTest(change=change), self.assertRaisesRegex(ValueError, "next"):
                validate_profiles(self.changed(*change))

    def test_device_names_are_refused_but_names_that_merely_start_like_one_are_not(self):
        for fine in ("Console Next", "Auxiliary Next", "COM10 Next", "Next Soyeht"):
            with self.subTest(folder=fine):
                document = self.changed("profiles", "dev", "displayNames", "windowsFolder", fine)
                validate_profiles(document)

    def test_the_windows_install_folder_is_pinned(self):
        # Never change it: the login tasks of a release hold the absolute path of each program in
        # this folder (contract 13), so a new name would strand every registration.
        names = self.document["profiles"]["prod"]["displayNames"]
        self.assertEqual(names["windowsFolder"], "Soyeht Next")

    def test_profiles_keep_their_state_apart(self):
        profiles = self.document["profiles"]
        self.assertEqual(profiles["dev"]["tcpPort"], 8291)
        self.assertEqual(profiles["prod"]["tcpPort"], 8290)
        for system, path in profiles["prod"]["dataDirectory"].items():
            self.assertTrue(profiles["dev"]["dataDirectory"][system].startswith(path + "/"))

    def test_profile_values_are_not_typed_again_in_runtime_sources(self):
        values = {self.document["sockets"]["engine"], self.document["sockets"]["ptyd"]}
        for profile in self.document["profiles"].values():
            values.add("Library/Application Support/" + profile["dataDirectory"]["macos"])
            values.add(str(profile["tcpPort"]))
            values |= set(profile["launchdLabels"].values())
            values |= set(profile["systemdUnits"].values())
            values |= set(profile["windowsTasks"].values())
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
            if path in generated or "test_" in path.name:
                continue
            source = path.read_text()
            for value in values:
                pattern = rf"(?<![A-Za-z0-9]){re.escape(value)}(?![A-Za-z0-9])"
                with self.subTest(path=path.relative_to(ROOT.parent), value=value):
                    self.assertIsNone(re.search(pattern, source))


if __name__ == "__main__":
    unittest.main()
