#!/usr/bin/env python3
"""Deterministic, offline generator for the documented schema vocabulary."""

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

from client_policy import render_client_policy
from design_tokens import TOKENS_PATH, render_tokens

ROOT = Path(__file__).resolve().parent.parent
SCHEMA_PATH = ROOT / "contracts/protocol.json"
PROFILES_PATH = ROOT / "contracts/profiles.json"
PROFILE_COMPONENTS = ("engine", "ptyd")


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


def rust_definition(name, schema):
    derive = "#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]"
    if "enum" in schema:
        fields = [f'    #[serde(rename = {json.dumps(v, ensure_ascii=False)})]\n    {variant(v)},' for v in schema["enum"]]
        return derive + f"\npub enum {name} {{\n" + "\n".join(fields) + "\n}"
    if "oneOf" in schema:
        fields = [f"    V{i}({type_name(t, 'rust')})," for i, t in enumerate(schema["oneOf"])]
        return derive + f"\n#[serde(untagged)]\npub enum {name} {{\n" + "\n".join(fields) + "\n}"
    if schema.get("type") == "object":
        fields = []
        for key, value in schema["properties"].items():
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
        return f"public enum {name}: String, Codable, Sendable {{\n" + "\n".join(fields) + "\n}"
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


def validate_profiles(document):
    if set(document) != {"version", "profiles"} or document["version"] != 1:
        raise ValueError("Profiles document must have version 1 and profiles")
    if set(document["profiles"]) != {"dev"}:
        raise ValueError("Exactly one Dev profile is required")
    profile = document["profiles"]["dev"]
    required = {"dataDirectoryFromHome", "sockets", "tcpPort", "launchdLabels"}
    if set(profile) != required:
        raise ValueError("Dev profile fields do not match the generated contract")
    data = profile["dataDirectoryFromHome"]
    prefix = "Library/Application Support/"
    if (
        not isinstance(data, str)
        or not data.isascii()
        or not data.isprintable()
        or "\\" in data
        or not data.startswith(prefix)
        or data.endswith("/")
    ):
        raise ValueError("Dev data directory must be relative to the macOS home directory")
    if any(part in {"", ".", ".."} for part in data.split("/")):
        raise ValueError("Dev data directory has an unsafe component")
    port = profile["tcpPort"]
    if isinstance(port, bool) or not isinstance(port, int) or not 1 <= port <= 65535:
        raise ValueError("Dev TCP port must be between 1 and 65535")
    for field, suffix in (("sockets", ".sock"), ("launchdLabels", None)):
        values = profile[field]
        if not isinstance(values, dict) or set(values) != set(PROFILE_COMPONENTS):
            raise ValueError(f"Dev {field} must name every component")
        if len(set(values.values())) != len(PROFILE_COMPONENTS):
            raise ValueError(f"Dev {field} values must be unique")
        for component, value in values.items():
            if not isinstance(value, str) or not value or not value.isascii():
                raise ValueError(f"Dev {field} contains an invalid value")
            if suffix and not re.fullmatch(r"[A-Za-z0-9._-]+\.sock", value):
                raise ValueError("Dev socket names must be .sock basenames")
            if field == "launchdLabels" and not re.fullmatch(
                rf"[a-z0-9]+(?:[.-][a-z0-9]+)*\.{component}", value
            ):
                raise ValueError("Dev launchd label must end with its component")
    prefixes = {
        value.removesuffix(f".{component}")
        for component, value in profile["launchdLabels"].items()
    }
    if len(prefixes) != 1:
        raise ValueError("Dev launchd labels must share one namespace")


def profile_values(document):
    profile = document["profiles"]["dev"]
    data = profile["dataDirectoryFromHome"]
    labels = profile["launchdLabels"]
    return {
        "data": data,
        "application_support": data.removeprefix("Library/Application Support/"),
        "engine_socket": profile["sockets"]["engine"],
        "ptyd_socket": profile["sockets"]["ptyd"],
        "port": profile["tcpPort"],
        "engine_label": labels["engine"],
        "ptyd_label": labels["ptyd"],
        "label_prefix": labels["engine"].removesuffix(".engine"),
    }


def launchd_plist(component, label, digest):
    return f'''<?xml version="1.0" encoding="UTF-8"?>
<!-- Generated from contracts/profiles.json (sha256 {digest}). Do not edit. -->
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key><array><string>__BINARY__</string><string>--data-dir</string><string>__DATA__</string></array>
  <key>KeepAlive</key><true/>
  <key>RunAtLoad</key><true/>
  <key>ThrottleInterval</key><integer>1</integer>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardOutPath</key><string>__DATA__/logs/{component}.log</string>
  <key>StandardErrorPath</key><string>__DATA__/logs/{component}.log</string>
</dict></plist>
'''


def render_profiles(document, digest):
    value = profile_values(document)
    header = f"Generated from contracts/profiles.json (sha256 {digest}). Do not edit."
    rust = f'''// {header}
pub const DEV_DATA_DIRECTORY_FROM_HOME: &str = {json.dumps(value["data"])};
pub const DEV_ENGINE_SOCKET: &str = {json.dumps(value["engine_socket"])};
pub const DEV_PTYD_SOCKET: &str = {json.dumps(value["ptyd_socket"])};
pub const DEV_TCP_PORT: u16 = {value["port"]};
pub const DEV_ENGINE_LAUNCHD_LABEL: &str = {json.dumps(value["engine_label"])};
pub const DEV_PTYD_LAUNCHD_LABEL: &str = {json.dumps(value["ptyd_label"])};
'''
    swift = f'''// {header}
public enum DevProfileValues {{
    public static let dataDirectoryFromHome = {json.dumps(value["data"])}
    public static let applicationSupportSubpath = {json.dumps(value["application_support"])}
    public static let engineSocketName = {json.dumps(value["engine_socket"])}
    public static let ptydSocketName = {json.dumps(value["ptyd_socket"])}
    public static let tcpPort: UInt16 = {value["port"]}
    public static let engineLaunchdLabel = {json.dumps(value["engine_label"])}
    public static let ptydLaunchdLabel = {json.dumps(value["ptyd_label"])}
}}
'''
    python = f'''# {header}
DEV_DATA_DIRECTORY_FROM_HOME = {value["data"]!r}
DEV_ENGINE_SOCKET = {value["engine_socket"]!r}
DEV_PTYD_SOCKET = {value["ptyd_socket"]!r}
DEV_SOCKET_NAMES = {{"engine": DEV_ENGINE_SOCKET, "ptyd": DEV_PTYD_SOCKET}}
DEV_TCP_PORT = {value["port"]}
DEV_ENGINE_LAUNCHD_LABEL = {value["engine_label"]!r}
DEV_PTYD_LAUNCHD_LABEL = {value["ptyd_label"]!r}
DEV_LAUNCHD_LABELS = {{"engine": DEV_ENGINE_LAUNCHD_LABEL, "ptyd": DEV_PTYD_LAUNCHD_LABEL}}
DEV_LAUNCHD_PREFIX = {value["label_prefix"]!r}
'''
    return {
        "engine/soyeht-protocol/src/profile.rs": rust,
        "contracts/swift/Sources/SoyehtProtocol/Profile.swift": swift,
        "scripts/generated_profile.py": python,
        "engine/soyeht-engine/launchd/dev.plist": launchd_plist(
            "engine", value["engine_label"], digest
        ),
        "engine/soyeht-ptyd/launchd/dev.plist": launchd_plist(
            "ptyd", value["ptyd_label"], digest
        ),
    }


def render(schema, digest):
    outputs = {}
    outputs["contracts/typescript/src/schema.json"] = json.dumps(schema, ensure_ascii=False, indent=2) + "\n"
    header = f"// Generated from contracts/protocol.json (sha256 {digest}). Do not edit.\n"
    rust = header + "\n".join(rust_definition(n, s) for n, s in schema["$defs"].items())
    rust += "\n" + registry_rust(schema) + "\n" + event_registry_rust(schema) + "\n" + rust_names(schema) + "\n"
    rust += "\npub fn error_code(kind: &ErrorKind) -> i64 {\n    match kind {\n" + "\n".join(f"        ErrorKind::{variant(name)} => {code}," for name,code in schema["x-errorCodes"].items()) + "\n    }\n}\n"
    # Formatting in memory keeps --check read-only and identical across invocations.
    rust = subprocess.run(["rustfmt", "--edition", "2024"], input=rust, text=True, capture_output=True, check=True).stdout
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
    constants += "\n".join(f"pub const {snake(k).upper()}: usize = {v};" for k, v in schema["x-limits"].items()) + "\n"
    constants += "\n".join(f"pub const FRAME_KIND_{k.upper()}: u8 = {v};" for k, v in frame_kinds(schema).items()) + "\n"
    outputs["engine/soyeht-protocol/src/limits.rs"] = constants
    outputs["contracts/typescript/src/limits.ts"] = header + "export const limits = " + json.dumps(schema["x-limits"], indent=2) + " as const;\n"
    outputs["contracts/swift/Sources/SoyehtProtocol/Limits.swift"] = header + "public enum ProtocolLimits {\n" + "\n".join(f"    public static let {k} = {v}" for k, v in schema["x-limits"].items()) + "\n}\n\npublic enum FrameKind {\n" + "\n".join(f"    public static let {k}: UInt8 = {v}" for k, v in frame_kinds(schema).items()) + "\n}\n"
    outputs.update(render_client_policy(schema, header))
    # Round-trip executables dispatch through every named schema, not through untyped JSON.
    swift_cases = []
    rust_cases = []
    for name in schema["$defs"]:
        swift_name = "TaskRecord" if name == "Task" else name
        swift_cases.append(f'        case "{name}": return try codec(SoyehtProtocol.{swift_name}.self, data)')
        rust_cases.append(f'        "{name}" => roundtrip::<{name}>(value),')
    outputs["contracts/swift/Sources/ProtocolVectors/Dispatch.swift"] = header + "import Foundation\nimport SoyehtProtocol\n\nfunc roundtrip(_ name: String, _ data: Data) throws -> Data {\n    switch name {\n" + "\n".join(swift_cases) + '\n    default: throw NSError(domain: "Unknown schema", code: 1)\n    }\n}\n'
    outputs["engine/soyeht-protocol/tests/support/dispatch.rs"] = header + "fn dispatch(name: &str, value: serde_json::Value) -> serde_json::Value {\n    match name {\n" + "\n".join(rust_cases) + '\n        _ => panic!("unknown schema: {name}"),\n    }\n}\n'
    return outputs


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    data = SCHEMA_PATH.read_bytes()
    schema = json.loads(data)
    validate_schema(schema)
    outputs = render(schema, hashlib.sha256(data).hexdigest())
    profile_data = PROFILES_PATH.read_bytes()
    profiles = json.loads(profile_data)
    validate_profiles(profiles)
    outputs.update(render_profiles(profiles, hashlib.sha256(profile_data).hexdigest()))
    token_data = (ROOT / TOKENS_PATH).read_bytes()
    outputs.update(render_tokens(json.loads(token_data), token_data))
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
