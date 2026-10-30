#!/usr/bin/env python3
"""Deterministic, offline generator for the documented schema vocabulary."""

import argparse
import json
import re
from pathlib import Path

from client_policy import render_client_policy
from design_tokens import TOKENS_PATH, render_tokens
from generated_header import header_text, rustfmt
from legacy import LEGACY_SOURCE, render_legacy, validate_legacy
from profile_contract import PROFILES_SOURCE, render_profiles, validate_profiles
from product_version import RELEASE_SOURCE, load as load_version, render as render_version
from readiness import render_readiness
from settings import render_settings

ROOT = Path(__file__).resolve().parent.parent
SCHEMA_SOURCE = "contracts/protocol.json"
SCHEMA_PATH = ROOT / SCHEMA_SOURCE
PROFILES_PATH = ROOT / PROFILES_SOURCE


def snake(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def variant(value):
    name = "".join(part[:1].upper() + part[1:] for part in re.split(r"[_\W]+", value) if part)
    return "V" + name if name[0].isdigit() else name


def lower_camel(value):
    name = variant(value)
    return name[:1].lower() + name[1:]


def type_name(schema, lang):
    if "$ref" in schema:
        name = schema["$ref"].split("/")[-1]
        return "TaskRecord" if lang == "swift" and name == "Task" else name
    if not schema:
        return "JsonValue"
    kind = schema["type"]
    primitives = {
        "rust": {"string": "String", "integer": "i64", "number": "f64", "boolean": "bool", "null": "()"},
        "swift": {"string": "String", "integer": "Int64", "number": "Double", "boolean": "Bool", "null": "JsonNull"},
        "ts": {"string": "string", "integer": "number", "number": "number", "boolean": "boolean", "null": "null"},
    }
    if kind == "array":
        inner = type_name(schema["items"], lang)
        return {"rust": f"Vec<{inner}>", "swift": f"[{inner}]", "ts": f"Array<{inner}>"}[lang]
    if kind == "integer" and schema.get("minimum", -1) >= 0:
        return {"rust": "u32", "swift": "UInt32", "ts": "number"}[lang]
    if kind not in primitives[lang]:
        raise ValueError(f"unsupported inline type: {schema}")
    return primitives[lang][kind]


def rust_definition(name, schema, skip=(), derive="#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]"):
    if "enum" in schema:
        fields = [f'    #[serde(rename = {json.dumps(v, ensure_ascii=False)})]\n    {variant(v)},' for v in schema["enum"]]
        return derive + f"\npub enum {name} {{\n" + "\n".join(fields) + "\n}"
    if "oneOf" in schema:
        fields = [f"    V{i}({type_name(t, 'rust')})," for i, t in enumerate(schema["oneOf"])]
        return derive + f"\n#[serde(untagged)]\npub enum {name} {{\n" + "\n".join(fields) + "\n}"
    if schema.get("type") == "object":
        fields = []
        for key, value in schema["properties"].items():
            if key in skip:
                continue
            kind = type_name(value, "rust")
            attrs = f'    #[serde(rename = "{key}")]'
            if key not in schema["required"]:
                kind = f"Option<{kind}>"
                attrs = f'    #[serde(rename = "{key}", skip_serializing_if = "Option::is_none")]'
            field = snake(key)
            if field in {"type", "ref", "self", "move", "loop", "mod", "match", "where"}:
                field = "r#" + field
            fields.append(f"{attrs}\n    pub {field}: {kind},")
        return derive + f"\npub struct {name} {{\n" + "\n".join(fields) + "\n}"
    return f"pub type {name} = " + ("serde_json::Value" if not schema else type_name(schema, "rust")) + ";"


def swift_definition(name, schema):
    name = "TaskRecord" if name == "Task" else name
    if "enum" in schema:
        fields = [f'    case `{snake(variant(v))}` = {json.dumps(v, ensure_ascii=False)}' for v in schema["enum"]]
        return f"public enum {name}: String, Codable, Sendable, CaseIterable {{\n" + "\n".join(fields) + "\n}"
    if "oneOf" in schema:
        types = [type_name(t, "swift") for t in schema["oneOf"]]
        fields = [f"    case v{i}({t})" for i, t in enumerate(types)]
        decodes = [f"        if let value = try? container.decode({t}.self) {{ self = .v{i}(value); return }}" for i, t in enumerate(types)]
        encodes = [f"        case .v{i}(let value): try container.encode(value)" for i in range(len(types))]
        return f"""public enum {name}: Codable, Sendable {{
{chr(10).join(fields)}
    public init(from decoder: any Decoder) throws {{
        let container = try decoder.singleValueContainer()
{chr(10).join(decodes)}
        throw DecodingError.dataCorruptedError(in: container, debugDescription: "No matching {name} variant")
    }}
    public func encode(to encoder: any Encoder) throws {{
        var container = encoder.singleValueContainer()
        switch self {{
{chr(10).join(encodes)}
        }}
    }}
}}"""
    if schema.get("type") == "object":
        fields, args, assigns = [], [], []
        for key, value in schema["properties"].items():
            kind = type_name(value, "swift")
            optional = key not in schema["required"]
            if optional:
                kind += "?"
            fields.append(f"    public var `{key}`: {kind}")
            args.append(f"`{key}`: {kind}" + (" = nil" if optional else ""))
            assigns.append(f"        self.`{key}` = `{key}`")
        return f"public struct {name}: Codable, Sendable {{\n" + "\n".join(fields) + f"\n    public init({', '.join(args)}) {{\n" + "\n".join(assigns) + "\n    }\n}"
    if not schema:
        return ""  # Handwritten JSON value implementation is shared by the generated models.
    return f"public typealias {name} = {type_name(schema, 'swift')}"


def ts_definition(name, schema):
    if "enum" in schema:
        return f"export type {name} = " + " | ".join(json.dumps(v, ensure_ascii=False) for v in schema["enum"]) + ";"
    if "oneOf" in schema:
        return f"export type {name} = " + " | ".join(type_name(t, "ts") for t in schema["oneOf"]) + ";"
    if schema.get("type") == "object":
        fields = [f'  {json.dumps(k)}{"?" if k not in schema["required"] else ""}: {type_name(v, "ts")};' for k, v in schema["properties"].items()]
        return f"export interface {name} {{\n" + "\n".join(fields) + "\n}"
    if not schema:
        return "export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };"
    return f"export type {name} = {type_name(schema, 'ts')};"


def pane_route_property(schema, method):
    params = schema["$defs"][method["params"]["$ref"].split("/")[-1]]
    if "x-paneRoute" in method:
        return method["x-paneRoute"]
    if "paneId" in params.get("properties", {}):
        return "paneId"
    return None


def registry_rust(schema):
    cases, rows, pane_routes = [], [], []
    for name, method in schema["x-methods"].items():
        kind = type_name(method["params"], "rust")
        case = variant(name)
        cases.append(f'    #[serde(rename = "{name}")]\n    {case}({kind}),')
        rows.append(f'    ("{name}", "{kind}", "{type_name(method["result"], "rust")}", {str(method["mutation"]).lower()}),')
        route = pane_route_property(schema, method)
        if route is not None:
            pane_routes.append(f'    ("{name}", "{route}"),')
    return "#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]\n#[serde(tag = \"method\", content = \"params\")]\npub enum Request {\n" + "\n".join(cases) + "\n}\npub const METHODS: &[(&str, &str, &str, bool)] = &[\n" + "\n".join(rows) + "\n];\npub const PANE_ROUTE_METHODS: &[(&str, &str)] = &[\n" + "\n".join(pane_routes) + "\n];"


STORE_ASSIGNED = ("eventId", "at", "actor")


def event_params_rust(schema):
    """One struct per event the engine builds, and the trait that names its event.

    A durable event gets its `eventId`, `at` and `actor` from the store, so the engine builds the struct
    without them (`...Draft`); a live event has neither and is built as its own schema.
    """
    uses = {}
    for name, event in schema["x-events"].items():
        uses.setdefault(event["params"]["$ref"].rsplit("/", 1)[1], []).append(name)
    out = [
        """/// The parameters of one event as the engine builds them.
pub trait EventParams: serde::Serialize {
    const NAME: EventName;
    /// The event name and its JSON parameters, ready for the store or the wire.
    fn to_event(&self) -> Result<(String, serde_json::Value), serde_json::Error> {
        Ok((Self::NAME.as_str().to_owned(), serde_json::to_value(self)?))
    }
}"""
    ]
    skipped = []
    for type_, names in uses.items():
        definition = schema["$defs"][type_]
        if len(names) != 1 or definition.get("type") != "object":
            reason = "share one params type" if len(names) > 1 else "has union params"
            skipped.append(f"// {', '.join(names)}: {reason} ({type_}), so the engine builds it as JSON.")
            continue
        event = schema["x-events"][names[0]]
        derive = "#[derive(Debug, Clone, PartialEq, serde::Serialize)]"
        if event["durable"]:
            target = type_.removesuffix("Event") + "Draft"
            # An `actor` the event requires is its own (a task movement's); the store's is optional.
            skip = tuple(k for k in STORE_ASSIGNED if k != "actor" or k not in definition["required"])
            out.append(rust_definition(target, definition, skip, derive))
        else:
            target = type_
        out.append(f"impl EventParams for {target} {{\n    const NAME: EventName = EventName::{variant(names[0])};\n}}")
    return "\n".join(["// Events with no `EventParams` type:", *skipped, *out])


def event_registry_rust(schema):
    cases = []
    names = []
    rows = []
    for name, event in schema["x-events"].items():
        case = variant(name)
        params = type_name(event["params"], "rust")
        durable = str(event["durable"]).lower()
        cases.append(f'    #[serde(rename = "{name}")]\n    {case},')
        names.append(f'            Self::{case} => "{name}",')
        rows.append(
            "    EventDescriptor { "
            f"name: EventName::{case}, params_type: \"{params}\", durable: {durable} "
            "},"
        )
    return f"""#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum EventName {{
{chr(10).join(cases)}
}}
impl EventName {{
    pub const fn as_str(self) -> &'static str {{
        match self {{
{chr(10).join(names)}
        }}
    }}
}}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventDescriptor {{
    pub name: EventName,
    pub params_type: &'static str,
    pub durable: bool,
}}
pub const EVENTS: &[EventDescriptor] = &[
{chr(10).join(rows)}
];"""


def event_registry_swift(schema):
    cases = []
    rows = []
    for name, event in schema["x-events"].items():
        case = lower_camel(name)
        params = type_name(event["params"], "swift")
        durable = str(event["durable"]).lower()
        cases.append(f'    case {case} = "{name}"')
        rows.append(
            f'    .{case}: EventDescriptor(paramsType: "{params}", durable: {durable}),'
        )
    return f"""public enum EventName: String, Codable, Sendable, CaseIterable {{
{chr(10).join(cases)}
}}

public struct EventDescriptor: Sendable {{
    public let paramsType: String
    public let durable: Bool
    public init(paramsType: String, durable: Bool) {{
        self.paramsType = paramsType
        self.durable = durable
    }}
}}

public let eventRegistry: [EventName: EventDescriptor] = [
{chr(10).join(rows)}
]"""


def event_registry_ts(schema):
    types = []
    rows = []
    for name, event in schema["x-events"].items():
        params = type_name(event["params"], "ts")
        durable = str(event["durable"]).lower()
        types.append(f'  "{name}": {params};')
        rows.append(
            f'  "{name}": {{ paramsType: "{params}", durable: {durable} }},'
        )
    return f"""export interface Events {{
{chr(10).join(types)}
}}
export type EventName = keyof Events;
export interface EventDescriptor {{
  paramsType: string;
  durable: boolean;
}}
export const eventRegistry = {{
{chr(10).join(rows)}
}} as const satisfies Record<EventName, EventDescriptor>;"""


def rust_names(schema):
    """String constants for every method and event name: usable in `match` patterns."""

    def module(kind, names):
        consts = [f'    pub const {snake(variant(n)).upper()}: &str = "{n}";' for n in names]
        return f"pub mod {kind} {{\n" + "\n".join(consts) + "\n}"

    return module("method", schema["x-methods"]) + "\n" + module("event", schema["x-events"])


def method_registry_swift(schema):
    cases = [f'    case {lower_camel(name)} = "{name}"' for name in schema["x-methods"]]
    return (
        "public enum RequestMethod: String, Codable, Sendable, CaseIterable {\n"
        + "\n".join(cases)
        + "\n}"
    )


def doc_line(notes, key, indent):
    """The doc comment line of a limit that has a note in `x-limitNotes`, or nothing."""
    return f"{indent}/// {notes[key]}\n" if key in notes else ""


def frame_kinds(schema):
    return schema["x-framing"]["types"]


def validate_schema(schema):
    allowed = {"$ref", "type", "properties", "required", "additionalProperties",
               "enum", "oneOf", "items", "minimum", "maximum", "minLength",
               "maxLength", "pattern", "contentEncoding", "description"}

    def visit(node):
        unknown = set(node) - allowed
        if unknown:
            raise ValueError(f"Unsupported schema keywords: {sorted(unknown)}")
        if "$ref" in node:
            if node["$ref"].removeprefix("#/$defs/") not in schema["$defs"]:
                raise ValueError("Unknown schema reference")
        if node.get("type") == "object":
            if node.get("additionalProperties") is not True:
                raise ValueError("Readers must accept unknown fields")
            if not set(node["required"]) <= set(node["properties"]):
                raise ValueError("Required field has no type")
            for child in node["properties"].values():
                visit(child)
        for child in node.get("oneOf", []):
            visit(child)
        if "items" in node:
            visit(node["items"])

    for name, definition in schema["$defs"].items():
        if not re.fullmatch(r"[A-Z][A-Za-z0-9]*", name):
            raise ValueError("Invalid type name")
        visit(definition)
    pane_non_route_ids = schema["x-paneNonRouteIds"]
    if (
        not isinstance(pane_non_route_ids, list)
        or any(not isinstance(field, str) for field in pane_non_route_ids)
        or len(pane_non_route_ids) != len(set(pane_non_route_ids))
    ):
        raise ValueError("Pane non-route identifiers must be unique names")
    pane_non_route_ids = set(pane_non_route_ids)
    used_pane_non_route_ids = set()
    for name, method in schema["x-methods"].items():
        allowed_method_keys = {"params", "result", "mutation", "x-paneRoute"}
        if set(method) - allowed_method_keys:
            raise ValueError(f"Unsupported method metadata for {name}")
        if type(method["mutation"]) is not bool:
            raise ValueError(f"Method mutation must be boolean for {name}")
        visit(method["params"])
        visit(method["result"])
        params = schema["$defs"][method["params"]["$ref"].split("/")[-1]]
        properties = params.get("properties", {})
        identifier_names = {
            field
            for field, rule in properties.items()
            if rule == {"$ref": "#/$defs/Id"}
            or (
                rule.get("type") == "array"
                and rule.get("items") == {"$ref": "#/$defs/Id"}
            )
        }
        route = pane_route_property(schema, method)
        if route is not None:
            if not isinstance(route, str):
                raise ValueError(f"Pane route must be a parameter name for {name}")
            if route not in properties or properties[route] != {"$ref": "#/$defs/Id"}:
                raise ValueError(f"Pane route must name an Id parameter for {name}")
        classified = identifier_names & pane_non_route_ids
        used_pane_non_route_ids.update(classified)
        if route is not None:
            classified.add(route)
        if classified != identifier_names:
            raise ValueError(f"Every Id parameter needs a pane-route decision for {name}")
    if used_pane_non_route_ids != pane_non_route_ids:
        raise ValueError("Pane non-route identifier list contains unused names")
    event_cases = []
    for name, event in schema["x-events"].items():
        if set(event) != {"params", "durable"} or type(event["durable"]) is not bool:
            raise ValueError("Event must have params and a boolean durable flag")
        if not re.fullmatch(r"[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+", name):
            raise ValueError("Invalid event name")
        visit(event["params"])
        event_cases.append(variant(name))
    if len(event_cases) != len(set(event_cases)):
        raise ValueError("Event names collide after identifier generation")
    method_cases = [variant(name) for name in schema["x-methods"]]
    if len(method_cases) != len(set(method_cases)):
        raise ValueError("Method names collide after identifier generation")
    if set(frame_kinds(schema)) != {"json", "terminal", "attachment"}:
        raise ValueError("Framing needs exactly the json, terminal and attachment kinds")

    error_kinds = schema["$defs"]["ErrorKind"].get("enum", [])
    error_codes = schema["x-errorCodes"]
    if list(error_codes) != error_kinds:
        raise ValueError("ErrorKind and x-errorCodes must have the same ordered names")
    if any(type(code) is not int for code in error_codes.values()):
        raise ValueError("Error codes must be integers")
    if len(set(error_codes.values())) != len(error_codes):
        raise ValueError("Error codes must be unique")

    model_source = schema["$defs"].get("ModelListSource", {})
    model_result_source = schema["$defs"].get("ModelListResult", {}).get(
        "properties", {}
    ).get("source")
    if not model_source.get("enum") or model_result_source != {
        "$ref": "#/$defs/ModelListSource"
    }:
        raise ValueError("ModelListResult.source must use the closed ModelListSource enum")


def render(schema):
    outputs = {}
    outputs["contracts/typescript/src/schema.json"] = json.dumps(schema, ensure_ascii=False, indent=2) + "\n"
    header = f"// {header_text(SCHEMA_SOURCE)}\n"
    rust = header + "\n".join(rust_definition(n, s) for n, s in schema["$defs"].items())
    rust += "\n" + registry_rust(schema) + "\n" + event_registry_rust(schema) + "\n" + event_params_rust(schema) + "\n" + rust_names(schema) + "\n"
    rust += "\npub fn error_code(kind: &ErrorKind) -> i64 {\n    match kind {\n" + "\n".join(f"        ErrorKind::{variant(name)} => {code}," for name,code in schema["x-errorCodes"].items()) + "\n    }\n}\n"
    # Formatting in memory keeps --check read-only and identical across invocations.
    rust = rustfmt(rust)
    outputs["engine/soyeht-protocol/src/generated.rs"] = rust
    swift = header + "import Foundation\n\n"
    swift += "\n\n".join(swift_definition(n, s) for n, s in schema["$defs"].items())
    swift += "\n\n" + method_registry_swift(schema) + "\n\n" + event_registry_swift(schema) + "\n"
    outputs["contracts/swift/Sources/SoyehtProtocol/Generated.swift"] = swift
    ts = header + "\n\n".join(ts_definition(n, s) for n, s in schema["$defs"].items())
    ts += "\n\nexport interface Methods {\n" + "\n".join(f'  "{n}": {{ params: {type_name(m["params"], "ts")}; result: {type_name(m["result"], "ts")} }};' for n, m in schema["x-methods"].items()) + "\n}\n"
    ts += "\n" + event_registry_ts(schema) + "\n"
    outputs["contracts/typescript/src/generated.ts"] = ts
    constants = header + f'pub const PROTOCOL_VERSION: u32 = {schema["x-protocolVersion"]};\n'
    notes = schema.get("x-limitNotes", {})
    constants += "\n".join(f"{doc_line(notes, k, '')}pub const {snake(k).upper()}: usize = {v};" for k, v in schema["x-limits"].items()) + "\n"
    constants += "\n".join(f"pub const FRAME_KIND_{k.upper()}: u8 = {v};" for k, v in frame_kinds(schema).items()) + "\n"
    outputs["engine/soyeht-protocol/src/limits.rs"] = constants
    outputs["contracts/typescript/src/limits.ts"] = header + "export const limits = " + json.dumps(schema["x-limits"], indent=2) + " as const;\n"
    outputs["contracts/swift/Sources/SoyehtProtocol/Limits.swift"] = header + "public enum ProtocolLimits {\n" + "\n".join(f"{doc_line(notes, k, '    ')}    public static let {k} = {v}" for k, v in schema["x-limits"].items()) + "\n}\n\npublic enum ProtocolVersion {\n    public static let current: UInt32 = " + str(schema["x-protocolVersion"]) + "\n}\n\npublic enum FrameKind {\n" + "\n".join(f"    public static let {k}: UInt8 = {v}" for k, v in frame_kinds(schema).items()) + "\n}\n"
    outputs.update(render_client_policy(schema, header))
    outputs.update(render_readiness(schema, header))
    outputs.update(render_settings(schema, header))
    # Round-trip executables dispatch through every named schema, not through untyped JSON.
    swift_cases = []
    rust_cases = []
    for name in schema["$defs"]:
        swift_name = "TaskRecord" if name == "Task" else name
        swift_cases.append(f'        case "{name}": return try codec(SoyehtProtocol.{swift_name}.self, data)')
        rust_cases.append(f'        "{name}" => decode_encode::<{name}>(name, value),')
    outputs["contracts/swift/Sources/ProtocolVectors/Dispatch.swift"] = header + "import Foundation\nimport SoyehtProtocol\n\nfunc roundtrip(_ name: String, _ data: Data) throws -> Data {\n    switch name {\n" + "\n".join(swift_cases) + '\n    default: throw NSError(domain: "Unknown schema", code: 1)\n    }\n}\n'
    outputs["engine/soyeht-protocol/src/contract/roundtrip.rs"] = rustfmt(
        header
        + "use super::{Mismatch, decode_encode};\nuse crate::generated::*;\nuse serde_json::Value;\n\n"
        + "pub(super) fn roundtrip(name: &str, value: &Value) -> Result<Value, Mismatch> {\n    match name {\n"
        + "\n".join(rust_cases)
        + "\n        _ => Err(Mismatch::Unknown(name.to_owned())),\n    }\n}\n"
    )
    return outputs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    data = SCHEMA_PATH.read_bytes()
    schema = json.loads(data)
    validate_schema(schema)
    outputs = render(schema)
    profile_data = PROFILES_PATH.read_bytes()
    profiles = json.loads(profile_data)
    validate_profiles(profiles)
    outputs.update(render_profiles(profiles))
    legacy = json.loads((ROOT / LEGACY_SOURCE).read_bytes())
    validate_legacy(legacy)
    outputs.update(render_legacy(legacy))
    outputs.update(render_version(load_version((ROOT / RELEASE_SOURCE).read_text())))
    token_data = (ROOT / TOKENS_PATH).read_bytes()
    outputs.update(render_tokens(json.loads(token_data)))
    mismatches = []
    for relative, content in outputs.items():
        path = ROOT / relative
        if args.check:
            if not path.exists() or path.read_text() != content:
                mismatches.append(relative)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
    if mismatches:
        raise SystemExit("Generated files differ:\n" + "\n".join(mismatches))


if __name__ == "__main__":
    main()
