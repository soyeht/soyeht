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


def render(schema, digest):
    outputs = {}
    outputs["contracts/typescript/src/schema.json"] = json.dumps(schema, ensure_ascii=False, indent=2) + "\n"
    header = f"// Generated from contracts/protocol.json (sha256 {digest}). Do not edit.\n"
    rust = header + "\n".join(rust_definition(n, s) for n, s in schema["$defs"].items()) + "\n" + registry_rust(schema) + "\n"
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
