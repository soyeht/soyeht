#!/usr/bin/env python3
"""Deterministic, offline generator for the documented schema vocabulary."""

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCHEMA_PATH = ROOT / "contracts/protocol.json"
PROFILES_PATH = ROOT / "contracts/profiles.json"
PROFILE_COMPONENTS = ("engine", "ptyd")


def snake(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def variant(value):
    name = "".join(part[:1].upper() + part[1:] for part in re.split(r"[_\W]+", value) if part)
    return "V" + name if name[0].isdigit() else name


def type_name(schema, lang):
    if "$ref" in schema:
        return schema["$ref"].split("/")[-1]
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


def registry_rust(schema):
    cases, rows = [], []
    for name, method in schema["x-methods"].items():
        kind = type_name(method["params"], "rust")
        case = variant(name)
        cases.append(f'    #[serde(rename = "{name}")]\n    {case}({kind}),')
        rows.append(f'    ("{name}", "{kind}", "{type_name(method["result"], "rust")}", {str(method["mutation"]).lower()}),')
    return "#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]\n#[serde(tag = \"method\", content = \"params\")]\npub enum Request {\n" + "\n".join(cases) + "\n}\npub const METHODS: &[(&str, &str, &str, bool)] = &[\n" + "\n".join(rows) + "\n];"


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
    for method in schema["x-methods"].values():
        visit(method["params"])
        visit(method["result"])


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
    rust = header + "\n".join(rust_definition(n, s) for n, s in schema["$defs"].items()) + "\n" + registry_rust(schema) + "\n"
    rust += "\npub fn error_code(kind: &ErrorKind) -> i64 {\n    match kind {\n" + "\n".join(f"        ErrorKind::{variant(name)} => {code}," for name,code in schema["x-errorCodes"].items()) + "\n    }\n}\n"
    # Formatting in memory keeps --check read-only and identical across invocations.
    rust = subprocess.run(["rustfmt", "--edition", "2024"], input=rust, text=True, capture_output=True, check=True).stdout
    outputs["engine/soyeht-protocol/src/generated.rs"] = rust
    outputs["contracts/swift/Sources/SoyehtProtocol/Generated.swift"] = header + "import Foundation\n\n" + "\n\n".join(swift_definition(n, s) for n, s in schema["$defs"].items()) + "\n"
    ts = header + "\n\n".join(ts_definition(n, s) for n, s in schema["$defs"].items())
    ts += "\n\nexport interface Methods {\n" + "\n".join(f'  "{n}": {{ params: {type_name(m["params"], "ts")}; result: {type_name(m["result"], "ts")} }};' for n, m in schema["x-methods"].items()) + "\n}\n"
    outputs["contracts/typescript/src/generated.ts"] = ts
    constants = header + f'pub const PROTOCOL_VERSION: u32 = {schema["x-protocolVersion"]};\n'
    constants += "\n".join(f"pub const {snake(k).upper()}: usize = {v};" for k, v in schema["x-limits"].items()) + "\n"
    outputs["engine/soyeht-protocol/src/limits.rs"] = constants
    outputs["contracts/typescript/src/limits.ts"] = header + "export const limits = " + json.dumps(schema["x-limits"], indent=2) + " as const;\n"
    outputs["contracts/swift/Sources/SoyehtProtocol/Limits.swift"] = header + "public enum ProtocolLimits {\n" + "\n".join(f"    public static let {k} = {v}" for k, v in schema["x-limits"].items()) + "\n}\n"
    # Round-trip executables dispatch through every named schema, not through untyped JSON.
    swift_cases = []
    rust_cases = []
    for name in schema["$defs"]:
        swift_cases.append(f'        case "{name}": return try codec(SoyehtProtocol.{name}.self, data)')
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
