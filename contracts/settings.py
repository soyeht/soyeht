"""Renders the `x-settings` section of protocol.json into the clients' and the engine's setting rules.

A setting is a key the engine stores and every client can read and change through `settings.get`
and `settings.set`. The section defines each key once, with its type, its default and its bounds.
The Rust protocol crate and the Swift protocol package get the keys, the defaults and the
validation as code, and the TypeScript reader gets the same table, so the engine and the clients
type no key and no bound by hand.

An entry whose key ends in `.<name>` is a family: the key is the prefix followed by a parameter
(a harness ID), and `parameter` bounds that part.
"""

import json
import re
import subprocess

RUST_OUTPUT = "engine/soyeht-protocol/src/setting.rs"
SWIFT_OUTPUT = "contracts/swift/Sources/SoyehtProtocol/Setting.swift"
TS_OUTPUT = "contracts/typescript/src/settings.ts"

KINDS = ("boolean", "integer", "string")
FIELDS = {
    "boolean": {"type", "default"},
    "integer": {"type", "default", "minimum", "maximum"},
    "string": {"type", "default", "maxLength", "parameter"},
}
KEY = re.compile(r"[a-z][A-Za-z0-9]*(?:\.[a-z][A-Za-z0-9]*)+")
FAMILY = re.compile(r"(?P<prefix>(?:[a-z][A-Za-z0-9]*\.)+)<(?P<name>[a-z][A-Za-z0-9]*)>")


def snake(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def upper_camel(name):
    return name[:1].upper() + name[1:]


def lower_camel(name):
    return name[:1].lower() + name[1:]


def parts(key):
    """The words of a dotted key, `terminal.defaultCols` -> ["terminal", "defaultCols"]."""
    return key.split(".")


def validate(settings):
    if not settings:
        raise ValueError("x-settings needs at least one setting")
    seen = set()
    for key, rule in settings.items():
        family = FAMILY.fullmatch(key)
        if not family and not KEY.fullmatch(key):
            raise ValueError(f"x-settings key {key!r} must be dotted words, or end in .<name>")
        kind = rule.get("type")
        if kind not in KINDS:
            raise ValueError(f"x-settings {key} has an unsupported type")
        if set(rule) - FIELDS[kind] or "default" not in rule:
            raise ValueError(f"x-settings {key} takes {sorted(FIELDS[kind])}, default required")
        default = rule["default"]
        if kind == "boolean" and type(default) is not bool:
            raise ValueError(f"x-settings {key} default must be a boolean")
        if kind == "integer":
            low, high = rule.get("minimum"), rule.get("maximum")
            if any(type(bound) is not int for bound in (low, high, default)) or not low <= default <= high:
                raise ValueError(f"x-settings {key} needs whole minimum <= default <= maximum")
        if kind == "string":
            limit = rule.get("maxLength")
            if type(default) is not str or type(limit) is not int or limit < 1:
                raise ValueError(f"x-settings {key} needs a string default and a maxLength")
            if len(default) > limit:
                raise ValueError(f"x-settings {key} default is longer than its maxLength")
        if "parameter" in rule:
            bounds = rule["parameter"]
            if not family or set(bounds) != {"minLength", "maxLength"}:
                raise ValueError(f"x-settings {key}: only a family takes a parameter, with its bounds")
            if not 1 <= bounds["minLength"] <= bounds["maxLength"]:
                raise ValueError(f"x-settings {key}: parameter bounds must be 1 <= min <= max")
        elif family:
            raise ValueError(f"x-settings {key} is a family and needs a parameter")
        identity = family["prefix"] if family else key
        if identity in seen:
            raise ValueError(f"x-settings key {key} appears twice")
        seen.add(identity)


def entries(settings):
    """Split the section into (single keys, families) as (key, rule) and (prefix, name, rule)."""
    singles, families = [], []
    for key, rule in settings.items():
        family = FAMILY.fullmatch(key)
        if family:
            families.append((family["prefix"], family["name"], rule))
        else:
            singles.append((key, rule))
    return singles, families


def rust_name(key):
    return "_".join(snake(word) for word in parts(key)).upper()


def rust_default(rule):
    kind = rule["type"]
    if kind == "boolean":
        return f"Value::Bool({str(rule['default']).lower()})"
    if kind == "integer":
        return f"Value::from({rule['default']})"
    return f"Value::String({json.dumps(rule['default'])}.into())"


def rust_check(rule, subject):
    """A boolean Rust expression: whether `subject`, a `&Value`, is acceptable."""
    kind = rule["type"]
    if kind == "boolean":
        return f"{subject}.is_boolean()"
    if kind == "integer":
        return (
            f"{subject}.as_i64().is_some_and(|n| ({rule['minimum']}..={rule['maximum']}).contains(&n))"
        )
    return f"{subject}.as_str().is_some_and(|text| text.len() <= {rule['maxLength']} && !text.contains('\\0'))"


def render_rust(settings, header):
    singles, families = entries(settings)
    lines = [
        header,
        "use serde_json::Value;",
        "",
        "/// Why the engine refuses a `settings.get` or `settings.set`.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        "pub enum SettingError {",
        "    /// The key is not one of `x-settings`.",
        "    Unknown,",
        "    /// The value has the wrong type or is out of bounds.",
        "    Invalid,",
        "}",
        "",
    ]
    for key, _ in singles:
        lines.append(f"pub const {rust_name(key)}: &str = {json.dumps(key)};")
    for prefix, _, _ in families:
        lines.append(f"pub const {rust_name(prefix.rstrip('.'))}_PREFIX: &str = {json.dumps(prefix)};")
    lines += [
        "",
        "/// Every setting that stands alone.",
        f"pub const KEYS: [&str; {len(singles)}] = [",
        *[f"    {rust_name(key)}," for key, _ in singles],
        "];",
        "",
    ]
    for prefix, name, rule in families:
        const = rust_name(prefix.rstrip("."))
        function = snake(prefix.rstrip(".").replace(".", "_")) + "_key"
        parameter = snake(name)
        low, high = rule["parameter"]["minLength"], rule["parameter"]["maxLength"]
        lines += [
            f"/// The key of the `{prefix}<{name}>` setting for `{parameter}`.",
            f"pub fn {function}({parameter}: &str) -> String {{",
            f'    format!("{{{const}_PREFIX}}{{{parameter}}}")',
            "}",
            "",
            f"/// The `{name}` of a `{prefix}<{name}>` key, when `key` is one.",
            f"pub fn {function}_parameter(key: &str) -> Option<&str> {{",
            f"    key.strip_prefix({const}_PREFIX)",
            f"        .filter(|{parameter}| ({low}..={high}).contains(&{parameter}.len()))",
            "}",
            "",
        ]
    arms = [f"        {rust_name(key)} => {rust_default(rule)}," for key, rule in singles]
    family_default = [
        f"        key if {snake(prefix.rstrip('.').replace('.', '_'))}_key_parameter(key).is_some() => {rust_default(rule)},"
        for prefix, _, rule in families
    ]
    lines += [
        "/// The value a setting has until someone changes it.",
        "pub fn default_value(key: &str) -> Result<Value, SettingError> {",
        "    Ok(match key {",
        *arms,
        *family_default,
        "        _ => return Err(SettingError::Unknown),",
        "    })",
        "}",
        "",
        "/// Whether `value` may be stored under `key`.",
        "pub fn validate(key: &str, value: &Value) -> Result<(), SettingError> {",
        "    let accepted = match key {",
        *[f"        {rust_name(key)} => {rust_check(rule, 'value')}," for key, rule in singles],
        *[
            f"        key if {snake(prefix.rstrip('.').replace('.', '_'))}_key_parameter(key).is_some() => {rust_check(rule, 'value')},"
            for prefix, _, rule in families
        ],
        "        _ => return Err(SettingError::Unknown),",
        "    };",
        "    if accepted { Ok(()) } else { Err(SettingError::Invalid) }",
        "}",
    ]
    source = "\n".join(lines) + "\n"
    return subprocess.run(
        ["rustfmt", "--edition", "2024"], input=source, text=True, capture_output=True, check=True
    ).stdout


def swift_name(key):
    words = parts(key)
    return words[0] + "".join(upper_camel(word) for word in words[1:])


def swift_default(rule):
    kind = rule["type"]
    if kind == "boolean":
        return f".bool({str(rule['default']).lower()})"
    if kind == "integer":
        return f".integer({rule['default']})"
    return f".string({json.dumps(rule['default'])})"


def swift_check(rule):
    """The statements of a Swift `case` that decide whether `value` is acceptable."""
    kind = rule["type"]
    if kind == "boolean":
        return ["            if case .bool = value { return true }", "            return false"]
    if kind == "integer":
        return [
            "            guard case .integer(let number) = value else { return false }",
            f"            return ({rule['minimum']}...{rule['maximum']}).contains(number)",
        ]
    return [
        "            guard case .string(let text) = value else { return false }",
        f'            return text.utf8.count <= {rule["maxLength"]} && !text.contains("\\0")',
    ]


def render_swift(settings, header):
    singles, families = entries(settings)
    lines = [
        header.rstrip("\n"),
        "",
        "/// The settings the engine stores, from `x-settings` in `contracts/protocol.json`.",
        "public enum Setting {",
    ]
    for key, _ in singles:
        lines.append(f"    public static let {swift_name(key)} = {json.dumps(key)}")
    for prefix, _, _ in families:
        lines.append(f"    public static let {swift_name(prefix.rstrip('.'))}Prefix = {json.dumps(prefix)}")
    lines += [
        "",
        "    /// Every setting that stands alone.",
        f"    public static let keys = [{', '.join(swift_name(key) for key, _ in singles)}]",
        "",
    ]
    for prefix, name, rule in families:
        base = swift_name(prefix.rstrip("."))
        low, high = rule["parameter"]["minLength"], rule["parameter"]["maxLength"]
        lines += [
            f"    /// The key of the `{prefix}<{name}>` setting for `{name}`.",
            f"    public static func {base}Key(_ {name}: String) -> String {{ {base}Prefix + {name} }}",
            "",
            f"    /// The `{name}` of a `{prefix}<{name}>` key, when `key` is one.",
            f"    public static func {base}Parameter(of key: String) -> String? {{",
            f"        guard key.hasPrefix({base}Prefix) else {{ return nil }}",
            f"        let {name} = String(key.dropFirst({base}Prefix.count))",
            f"        return ({low}...{high}).contains({name}.utf8.count) ? {name} : nil",
            "    }",
            "",
        ]
    default_arms = [f"        case {swift_name(key)}: {swift_default(rule)}" for key, rule in singles]
    for prefix, _, rule in families:
        default_arms.append(f"        case _ where {swift_name(prefix.rstrip('.'))}Parameter(of: key) != nil: {swift_default(rule)}")
    check_arms = []
    for key, rule in singles:
        check_arms += [f"        case {swift_name(key)}:", *swift_check(rule)]
    for prefix, _, rule in families:
        check_arms += [
            f"        case _ where {swift_name(prefix.rstrip('.'))}Parameter(of: key) != nil:",
            *swift_check(rule),
        ]
    lines += [
        "    /// The value a setting has until someone changes it; `nil` for a key that is not a setting.",
        "    public static func defaultValue(for key: String) -> JsonValue? {",
        "        switch key {",
        *default_arms,
        "        default: nil",
        "        }",
        "    }",
        "",
        "    /// Whether `value` may be stored under `key`.",
        "    public static func accepts(_ value: JsonValue, for key: String) -> Bool {",
        "        switch key {",
        *check_arms,
        "        default: return false",
        "        }",
        "    }",
        "}",
    ]
    return "\n".join(lines) + "\n"


def render_ts(settings, header):
    return header + "export const settings = " + json.dumps(settings, indent=2) + " as const;\n"


def render_settings(schema, header):
    """The generated files for protocol.json's `x-settings`, under the shared `header`."""
    settings = schema["x-settings"]
    validate(settings)
    return {
        RUST_OUTPUT: render_rust(settings, header),
        SWIFT_OUTPUT: render_swift(settings, header),
        TS_OUTPUT: render_ts(settings, header),
    }
