"""Renders the `x-clientPolicy` section of protocol.json into the clients' shared rules.

The timeouts, the terminal input chunk, the usage and context bands, the provider names and the
rule that tells a model, effort or permission selector apart are defined once there. The Rust
protocol crate, the Swift protocol package and the TypeScript reader each get a generated form;
no client keeps a copy of a number or a name.
"""

import json
import re
import subprocess

RUST_OUTPUT = "engine/soyeht-protocol/src/client_policy.rs"
SWIFT_OUTPUT = "contracts/swift/Sources/SoyehtProtocol/ClientPolicy.swift"
TS_OUTPUT = "contracts/typescript/src/clientPolicy.ts"

# The section's entries that are rules rather than numbers.
PROVIDER_NAMES = "usageProviderNames"
CONFIG_ROLES = "configRoles"
# The roles a configuration selector can have, and the Rust method, Rust category constant and
# Swift property of each.
ROLES = {
    "model": ("is_model", "MODEL_CATEGORY", "isModel"),
    "effort": ("is_effort", "EFFORT_CATEGORY", "isEffort"),
    "permission": ("is_permission_mode", "PERMISSION_MODE_CATEGORY", "isPermissionMode"),
}
ROLE_FIELDS = {"category", "legacyId"}
# The names of this group's entries are already complete: no group prefix.
UNPREFIXED_GROUP = "timeouts"
SCALAR_GROUPS = (
    "timeouts",
    "terminal",
    "usageBand",
    "contextBand",
    "usageRing",
    "usageWindow",
)


def snake(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def variant(value):
    return value[:1].upper() + value[1:]


def validate(policy, schema):
    expected = {*SCALAR_GROUPS, PROVIDER_NAMES, CONFIG_ROLES}
    if set(policy) != expected:
        raise ValueError(f"x-clientPolicy needs exactly {sorted(expected)}")
    for group in SCALAR_GROUPS:
        for key, value in policy[group].items():
            if isinstance(value, bool) or not isinstance(value, int | float | str):
                raise ValueError(f"x-clientPolicy.{group}.{key} must be a number or a string")
    for key, value in policy["timeouts"].items():
        if not key.endswith("Ms") or not isinstance(value, int) or value <= 0:
            raise ValueError(f"x-clientPolicy.timeouts.{key} must be positive whole milliseconds")
    providers = schema["$defs"]["UsageProvider"]["enum"]
    if list(policy[PROVIDER_NAMES]) != providers:
        raise ValueError("usageProviderNames must name every UsageProvider, in order")
    if set(policy[CONFIG_ROLES]) != set(ROLES):
        raise ValueError(f"configRoles must be exactly {sorted(ROLES)}")
    for role, rule in policy[CONFIG_ROLES].items():
        if "category" not in rule or set(rule) - ROLE_FIELDS:
            raise ValueError(f"configRoles.{role} takes {sorted(ROLE_FIELDS)}, category required")


def constants(policy):
    """The scalar entries as (group+key camelCase, value), in file order."""
    found = []
    for group in SCALAR_GROUPS:
        for key, value in policy[group].items():
            found.append((key if group == UNPREFIXED_GROUP else group + variant(key), value))
    return found


def rust_constant(name, value):
    if isinstance(value, str):
        return f"pub const {snake(name).upper()}: &str = {json.dumps(value)};"
    if isinstance(value, float):
        return f"pub const {snake(name).upper()}: f64 = {value!r};"
    kind = "usize" if name.endswith("Bytes") else "u64"
    return f"pub const {snake(name).upper()}: {kind} = {value};"


def role_rule_rust(role, rule):
    condition = f"self.category.as_deref().unwrap_or(&self.id) == {ROLES[role][1]}"
    if "legacyId" in rule:
        condition += f" || self.id == {json.dumps(rule['legacyId'])}"
    return condition


def render_rust(policy, header):
    lines = [header, "use crate::{ConfigOption, UsageProvider};", ""]
    lines += [rust_constant(name, value) for name, value in constants(policy)]
    lines += [
        f"pub const {category}: &str = {json.dumps(policy[CONFIG_ROLES][role]['category'])};"
        for role, (_, category, _) in ROLES.items()
    ]
    arms = "\n".join(
        f"            Self::{variant(provider)} => {json.dumps(name)},"
        for provider, name in policy[PROVIDER_NAMES].items()
    )
    lines += [
        "",
        "impl UsageProvider {",
        "    /// The name the clients show for the provider.",
        "    pub fn display_name(&self) -> &'static str {",
        "        match self {",
        arms,
        "        }",
        "    }",
        "}",
        "",
        "impl ConfigOption {",
        "    // A selector has a role when its category (its id when it has none) names it.",
        "",
    ]
    for role, (method, _, _) in ROLES.items():
        lines += [
            f"    /// Whether this option is the agent's {role} selector.",
            f"    pub fn {method}(&self) -> bool {{",
            f"        {role_rule_rust(role, policy[CONFIG_ROLES][role])}",
            "    }",
            "",
        ]
    lines.pop()
    lines.append("}")
    source = "\n".join(lines) + "\n"
    return subprocess.run(
        ["rustfmt", "--edition", "2024"], input=source, text=True, capture_output=True, check=True
    ).stdout


def swift_constant(name, value):
    if isinstance(value, str):
        return f"    public static let {name} = {json.dumps(value)}"
    if isinstance(value, float):
        return f"    public static let {name}: Double = {value!r}"
    line = f"    public static let {name} = {value}"
    if name.endswith("Ms"):
        line += (
            f"\n    public static let {name.removesuffix('Ms')}Seconds: Double = "
            f"{float(value) / 1000!r}"
        )
    return line


def role_rule_swift(rule):
    condition = f"(category ?? id) == {json.dumps(rule['category'])}"
    if "legacyId" in rule:
        condition += f" || id == {json.dumps(rule['legacyId'])}"
    return condition


def render_swift(policy, header):
    constant_lines = "\n".join(swift_constant(name, value) for name, value in constants(policy))
    arms = "\n".join(
        f"            case .{provider}: {json.dumps(name)}"
        for provider, name in policy[PROVIDER_NAMES].items()
    )
    roles = "\n\n".join(
        f"    /// Whether this option is the agent's {role} selector.\n"
        f"    public var {ROLES[role][2]}: Bool {{ {role_rule_swift(policy[CONFIG_ROLES][role])} }}"
        for role in ROLES
    )
    return (
        f"{header}"
        "\n"
        "/// The rules every client shares, from `x-clientPolicy` in `contracts/protocol.json`.\n"
        "public enum ClientPolicy {\n"
        f"{constant_lines}\n"
        "}\n\n"
        "extension UsageProvider {\n"
        "    /// The name the clients show for the provider.\n"
        "    public var displayName: String {\n"
        "        switch self {\n"
        f"{arms}\n"
        "        }\n"
        "    }\n"
        "}\n\n"
        "extension ConfigOption {\n"
        "    // A selector has a role when its category (its id when it has none) names it.\n"
        f"{roles}\n"
        "}\n"
    )


def render_ts(policy, header):
    return header + "export const clientPolicy = " + json.dumps(policy, indent=2) + " as const;\n"


def render_client_policy(schema, header):
    """The generated files for protocol.json's `x-clientPolicy`, under the shared `header`."""
    policy = schema["x-clientPolicy"]
    validate(policy, schema)
    return {
        RUST_OUTPUT: render_rust(policy, header),
        SWIFT_OUTPUT: render_swift(policy, header),
        TS_OUTPUT: render_ts(policy, header),
    }
