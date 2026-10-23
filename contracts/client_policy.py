"""Renders the `x-clientPolicy` section of protocol.json into the clients' shared rules.

The timeouts and the reconnect wait, the page sizes, the terminal input chunk, the usage and
context bands, the provider names, the texts two clients both store, the inline image types, what
a pane state means (whether a turn runs, the color of its dot) and the rule that tells a model,
effort or permission selector apart are defined once there. The Rust
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
# The names of these groups' entries are already complete: no group prefix.
UNPREFIXED_GROUPS = ("timeouts", "texts")
# The group of caps, the counts the clients stop at: messages read for a queue, links found in
# one text, pages of a model catalogue, tool calls the header ticker lists after the current one.
CAPS = "caps"
# The group of page sizes: each is a request `limit`, which the wire types as an unsigned 32-bit.
PAGE_SIZES = "pageSize"
# A page size that is the protocol's own maximum for the request, not a number of the clients.
AT_MAXIMUM = "maximum"
# The request whose `limit` each page size bounds.
PAGE_REQUESTS = {
    "queue": "PaneQueueListParams",
    "harnesses": "HarnessListParams",
    "models": "ModelListParams",
    "transcript": "PaneTranscriptParams",
    "tasks": "TaskListParams",
    "writingSection": "WritingSectionListParams",
}
INLINE_IMAGE_TYPES = "inlineImageTypes"
EDITOR_PAIRS = "editorPairs"
HIGHLIGHT_ROLES = "highlightRoles"
# The palette roles a syntax highlight can take, which each client resolves.
SYNTAX_ROLES = ("accent", "syntaxType", "warning", "muted", "success", "ink")
PANE_STATES = "paneStates"
STATE_FIELDS = {"turnRunning", "dot"}
# What a pane state's dot is colored from: a role of the palette, which each client resolves.
DOT_ROLES = ("success", "accent", "warning", "muted", "secondary")
SCALAR_GROUPS = (
    "timeouts",
    "terminal",
    CAPS,
    PAGE_SIZES,
    "texts",
    "usageBand",
    "contextBand",
    "usageRing",
    "usageWindow",
    "editor",
)


def snake(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def variant(value):
    return value[:1].upper() + value[1:]


def pascal(value):
    return "".join(part[:1].upper() + part[1:] for part in value.split("_"))


def resolved(policy, schema):
    """The policy with every page size that says `maximum` replaced by the request's maximum."""
    sizes = {
        key: (
            schema["$defs"][PAGE_REQUESTS[key]]["properties"]["limit"]["maximum"]
            if value == AT_MAXIMUM
            else value
        )
        for key, value in policy[PAGE_SIZES].items()
    }
    return {**policy, PAGE_SIZES: sizes}


def validate(policy, schema):
    expected = {
        *SCALAR_GROUPS,
        PROVIDER_NAMES,
        CONFIG_ROLES,
        INLINE_IMAGE_TYPES,
        EDITOR_PAIRS,
        HIGHLIGHT_ROLES,
        PANE_STATES,
    }
    if set(policy) != expected:
        raise ValueError(f"x-clientPolicy needs exactly {sorted(expected)}")
    for group in SCALAR_GROUPS:
        for key, value in policy[group].items():
            if isinstance(value, bool) or not isinstance(value, int | float | str):
                raise ValueError(f"x-clientPolicy.{group}.{key} must be a number or a string")
    for key, value in policy[CAPS].items():
        if type(value) is not int or value <= 0:
            raise ValueError(f"x-clientPolicy.{CAPS}.{key} must be a positive whole number")
    for key, value in policy["texts"].items():
        if not isinstance(value, str) or not value:
            raise ValueError(f"x-clientPolicy.texts.{key} must be a non-empty string")
    if set(policy[PAGE_SIZES]) != set(PAGE_REQUESTS):
        raise ValueError(f"pageSize needs exactly {sorted(PAGE_REQUESTS)}")
    for key, value in policy[PAGE_SIZES].items():
        limit = schema["$defs"][PAGE_REQUESTS[key]]["properties"]["limit"]
        if value != AT_MAXIMUM and (
            type(value) is not int or not limit["minimum"] <= value <= limit["maximum"]
        ):
            raise ValueError(f"pageSize.{key} must be {AT_MAXIMUM} or within the request's limit")
    types = policy[INLINE_IMAGE_TYPES]
    if not types or len(set(types)) != len(types) or not all(
        re.fullmatch(r"image/[a-z0-9.+-]+", kind) for kind in types
    ):
        raise ValueError("inlineImageTypes must list distinct image/* media types")
    pairs = policy[EDITOR_PAIRS]
    if not pairs or any(len(k) != 1 or len(v) != 1 for k, v in pairs.items()):
        raise ValueError("editorPairs maps one opening character to one closing character")
    if list(policy[HIGHLIGHT_ROLES]) != schema["$defs"]["HighlightKind"]["enum"]:
        raise ValueError("highlightRoles must name every HighlightKind, in order")
    if not set(policy[HIGHLIGHT_ROLES].values()) <= set(SYNTAX_ROLES):
        raise ValueError(f"highlightRoles take roles of {SYNTAX_ROLES}")
    states = schema["$defs"]["PaneState"]["enum"]
    if list(policy[PANE_STATES]) != states:
        raise ValueError("paneStates must name every PaneState, in order")
    for state, rule in policy[PANE_STATES].items():
        if set(rule) != STATE_FIELDS or type(rule["turnRunning"]) is not bool:
            raise ValueError(f"paneStates.{state} takes exactly {sorted(STATE_FIELDS)}")
        if rule["dot"] not in DOT_ROLES:
            raise ValueError(f"paneStates.{state}.dot must be one of {DOT_ROLES}")
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
            found.append((key if group in UNPREFIXED_GROUPS else group + variant(key), value))
    return found


def rust_constant(name, value):
    if isinstance(value, str):
        return f"pub const {snake(name).upper()}: &str = {json.dumps(value)};"
    if isinstance(value, float):
        return f"pub const {snake(name).upper()}: f64 = {value!r};"
    kind = (
        "usize"
        if name.endswith("Bytes") or name.startswith(CAPS)
        else "u32"
        if name.startswith((PAGE_SIZES, "editor"))
        else "u64"
    )
    return f"pub const {snake(name).upper()}: {kind} = {value};"


def role_rule_rust(role, rule):
    condition = f"self.category.as_deref().unwrap_or(&self.id) == {ROLES[role][1]}"
    if "legacyId" in rule:
        condition += f" || self.id == {json.dumps(rule['legacyId'])}"
    return condition


def render_rust(policy, header):
    lines = [header, "use crate::{ConfigOption, HighlightKind, PaneState, UsageProvider};", ""]
    lines += [rust_constant(name, value) for name, value in constants(policy)]
    types = policy[INLINE_IMAGE_TYPES]
    lines += [
        "/// Image types that claude-agent-acp and codex-acp forward as model input and that both",
        "/// providers accept; any other image is linked.",
        f"pub const INLINE_IMAGE_TYPES: &[&str] = &[{', '.join(map(json.dumps, types))}];",
    ]
    lines += [
        f"pub const {category}: &str = {json.dumps(policy[CONFIG_ROLES][role]['category'])};"
        for role, (_, category, _) in ROLES.items()
    ]
    arms = "\n".join(
        f"            Self::{variant(provider)} => {json.dumps(name)},"
        for provider, name in policy[PROVIDER_NAMES].items()
    )
    lines += render_pane_states_rust(policy[PANE_STATES])
    lines += render_editor_rust(policy)
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


def render_editor_rust(policy):
    pairs = policy[EDITOR_PAIRS]
    arms = ",\n".join(f"        {json.dumps(k)} => Some({json.dumps(v)})" for k, v in pairs.items())
    roles = ", ".join(pascal(role) for role in SYNTAX_ROLES)
    kinds = "\n".join(
        f"            Self::{pascal(kind)} => HighlightRole::{pascal(role)},"
        for kind, role in policy[HIGHLIGHT_ROLES].items()
    )
    return [
        "",
        "/// The character an editor types after `opening` to close the pair, when it closes one",
        "/// (brackets and quotes).",
        "pub fn editor_closing(opening: &str) -> Option<&'static str> {",
        "    match opening {",
        arms + ",",
        "        _ => None,",
        "    }",
        "}",
        "",
        "/// The palette role a syntax highlight is colored from; each client resolves the role.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        f"pub enum HighlightRole {{ {roles} }}",
        "",
        "impl HighlightKind {",
        "    /// The role of the highlight's color.",
        "    pub fn role(&self) -> HighlightRole {",
        "        match self {",
        kinds,
        "        }",
        "    }",
        "}",
    ]


def render_pane_states_rust(states):
    roles = ", ".join(pascal(role) for role in DOT_ROLES)
    def arms(render):
        return [f"            Self::{pascal(state)} => {render(rule)}," for state, rule in states.items()]

    running = " | ".join(f"Self::{pascal(state)}" for state, rule in states.items() if rule["turnRunning"])
    return [
        "",
        "/// The palette role a pane state's dot is colored from; each client resolves the role.",
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]",
        f"pub enum StateDot {{ {roles} }}",
        "",
        "impl PaneState {",
        "    /// The wire spelling of the state (`waiting_for_you`).",
        "    pub fn as_str(&self) -> &'static str {",
        "        match self {",
        *[f"            Self::{pascal(state)} => {json.dumps(state)}," for state in states],
        "        }",
        "    }",
        "",
        "    /// Whether an agent in this state is in the middle of a turn: it can be stopped, what",
        "    /// is typed is queued, and the header counts the time.",
        "    pub fn is_turn_running(&self) -> bool {",
        f"        matches!(self, {running})",
        "    }",
        "",
        "    /// The role of the state's dot.",
        "    pub fn dot(&self) -> StateDot {",
        "        match self {",
        *arms(lambda rule: f"StateDot::{pascal(rule['dot'])}"),
        "        }",
        "    }",
        "}",
    ]


def swift_constant(name, value):
    if isinstance(value, str):
        return f"    public static let {name} = {json.dumps(value)}"
    if isinstance(value, float):
        return f"    public static let {name}: Double = {value!r}"
    if name.startswith(PAGE_SIZES):
        return f"    public static let {name}: UInt32 = {value}"
    line = f"    public static let {name} = {value}"
    if name.endswith("Ms"):
        line += (
            f"\n    public static let {name.removesuffix('Ms')}Seconds: Double = "
            f"{float(value) / 1000!r}"
        )
    return line


def render_pane_states_swift(states):
    cases = ", ".join(DOT_ROLES)
    running = ", ".join(f".{state}" for state, rule in states.items() if rule["turnRunning"])
    dots = "\n".join(f"        case .{state}: .{rule['dot']}" for state, rule in states.items())
    return (
        "/// The palette role a pane state's dot is colored from; each client resolves the role.\n"
        "public enum StateDot: String, Sendable, CaseIterable {\n"
        f"    case {cases}\n"
        "}\n\n"
        "extension PaneState {\n"
        "    /// Whether an agent in this state is in the middle of a turn: it can be stopped, what\n"
        "    /// is typed is queued, and the header counts the time.\n"
        "    public var isTurnRunning: Bool {\n"
        "        switch self {\n"
        f"        case {running}: true\n"
        "        default: false\n"
        "        }\n"
        "    }\n\n"
        "    /// The role of the state's dot.\n"
        "    public var dot: StateDot {\n"
        "        switch self {\n"
        f"{dots}\n"
        "        }\n"
        "    }\n"
        "}"
    )


def render_editor_swift(policy):
    pairs = "\n".join(
        f"        case {json.dumps(k)}: {json.dumps(v)}" for k, v in policy[EDITOR_PAIRS].items()
    )
    roles = ", ".join(SYNTAX_ROLES)
    kinds = "\n".join(
        f"        case .{kind}: .{role}" for kind, role in policy[HIGHLIGHT_ROLES].items()
    )
    return (
        "extension ClientPolicy {\n"
        "    /// The character an editor types after `opening` to close the pair, when it closes one\n"
        "    /// (brackets and quotes).\n"
        "    public static func editorClosing(_ opening: String) -> String? {\n"
        "        switch opening {\n"
        f"{pairs}\n"
        "        default: nil\n"
        "        }\n"
        "    }\n"
        "}\n\n"
        "/// The palette role a syntax highlight is colored from; each client resolves the role.\n"
        "public enum HighlightRole: String, Sendable, CaseIterable {\n"
        f"    case {roles}\n"
        "}\n\n"
        "extension HighlightKind {\n"
        "    /// The role of the highlight's color.\n"
        "    public var role: HighlightRole {\n"
        "        switch self {\n"
        f"{kinds}\n"
        "        }\n"
        "    }\n"
        "}"
    )


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
    types = ", ".join(json.dumps(kind) for kind in policy[INLINE_IMAGE_TYPES])
    return (
        f"{header}"
        "\n"
        "/// The rules every client shares, from `x-clientPolicy` in `contracts/protocol.json`.\n"
        "public enum ClientPolicy {\n"
        f"{constant_lines}\n\n"
        "    /// Image types that claude-agent-acp and codex-acp forward as model input and that both\n"
        "    /// providers accept; any other image is linked.\n"
        f"    public static let inlineImageTypes = [{types}]\n"
        "}\n\n"
        f"{render_pane_states_swift(policy[PANE_STATES])}\n\n"
        f"{render_editor_swift(policy)}\n\n"
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
    policy = resolved(policy, schema)
    return {
        RUST_OUTPUT: render_rust(policy, header),
        SWIFT_OUTPUT: render_swift(policy, header),
        TS_OUTPUT: render_ts(policy, header),
    }
