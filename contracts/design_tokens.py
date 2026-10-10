"""Renders contracts/design-tokens.json into the Swift and Rust token files.

The palettes and the numeric tokens (radii, spacing, type scale, elevation, layout metrics,
fonts) are defined once in the JSON. The Mac client and the desktop client consume the
generated forms and keep no copy of their own.
"""

import hashlib
import json
import re
import subprocess

TOKENS_PATH = "contracts/design-tokens.json"
SWIFT_OUTPUT = "apps/shared/Sources/SoyehtClient/Generated.swift"
RUST_OUTPUT = "apps/desktop/crates/soyeht-desktop/src/theme/generated.rs"

HEX = re.compile(r"^#[0-9A-F]{6}$")
# The order of the palettes is the order of `TerminalColorTheme`.
PALETTE_NAMES = (
    "milk",
    "paleMist",
    "mistyBlue",
    "sunlitChartreuse",
    "sunriseGold",
    "deepVine",
    "deepForest",
    "midnightTeal",
    "deepHarbor",
)
COLOR_ROLES = (
    "screen",
    "canvas",
    "face",
    "hover",
    "well",
    "ink",
    "secondary",
    "muted",
    "accent",
    "onAccent",
    "shadowDark",
    "shadowLight",
    "wellShadow",
    "wellRim",
)
# Optional roles of a palette: a missing syntaxType or diffAdded falls back to an ANSI slot,
# exactly as `SoyehtThemePalette.make` does.
FALLBACKS = {"syntaxType": 13, "diffAdded": 10}
PALETTE_KEYS = set(COLOR_ROLES) | {
    "wellLip",
    "selection",
    "presentationLetterbox",
    "agents",
    "ansi",
    *FALLBACKS,
}
NUMERIC_SECTIONS = {
    "radius": "Radius",
    "space": "Space",
    "typeScale": "TypeScale",
    "paneShell": "PaneShell",
    "paneLayout": "PaneLayout",
    "layout": "Layout",
    "glyph": "Glyph",
    "usageRing": "UsageRing",
    "paneInterior": "PaneInterior",
    "composer": "Composer",
}
# Values only the desktop client has (window frame, popups, glyph sizes). They are Rust-only: the
# Mac output does not mention them, and it does not change when they do (see `mac_digest`).
DESKTOP_SECTION = "desktop"
ELEVATION_FIELDS = ("lightOffset", "lightBlur", "darkOffset", "darkBlur")


def snake(name):
    return re.sub(r"(?<=[a-z])(?=[A-Z])", "_", name).lower()


def _hex(value, where):
    if not isinstance(value, str) or not HEX.match(value):
        raise ValueError(f"{where}: {value!r} is not a canonical #RRGGBB color")


def _number(value, where):
    if isinstance(value, bool) or not isinstance(value, int | float):
        raise ValueError(f"{where}: {value!r} is not a number")


def validate_palette(name, palette):
    unknown = set(palette) - PALETTE_KEYS
    if unknown:
        raise ValueError(f"palette {name}: unknown keys {sorted(unknown)}")
    for role in (*COLOR_ROLES, "selection", "presentationLetterbox"):
        _hex(palette.get(role), f"palette {name} {role}")
    if "wellLip" not in palette:
        raise ValueError(f"palette {name}: wellLip is required (null when absent)")
    if palette["wellLip"] is not None:
        _hex(palette["wellLip"], f"palette {name} wellLip")
    ansi = palette.get("ansi", [])
    if len(ansi) != 16:
        raise ValueError(f"palette {name}: ansi needs 16 colors")
    if not 4 <= len(palette.get("agents", [])) <= 5:
        raise ValueError(f"palette {name}: agents needs 4 or 5 colors")
    for color in (*ansi, *palette["agents"]):
        _hex(color, f"palette {name} list")
    for role in FALLBACKS:
        if role in palette:
            _hex(palette[role], f"palette {name} {role}")


def validate_tokens(document):
    expected = {"palettes", "fonts", "elevation", DESKTOP_SECTION, *NUMERIC_SECTIONS}
    if set(document) != expected:
        raise ValueError(f"design tokens need exactly {sorted(expected)}")
    if list(document)[-1] != DESKTOP_SECTION:
        raise ValueError("the desktop section must be the last one")
    if tuple(document["palettes"]) != PALETTE_NAMES:
        raise ValueError(f"palettes must be {PALETTE_NAMES} in this order")
    for name, palette in document["palettes"].items():
        validate_palette(name, palette)
    for section in (*NUMERIC_SECTIONS, DESKTOP_SECTION):
        for key, value in document[section].items():
            _number(value, f"{section}.{key}")
    for key, value in document["fonts"].items():
        if not isinstance(value, str):
            raise ValueError(f"fonts.{key} must be a string")
    for name, entry in document["elevation"].items():
        if set(entry) == {"offset", "blur"}:
            _number(entry["offset"], name)
            _number(entry["blur"], name)
        elif set(entry) == set(ELEVATION_FIELDS):
            for value in entry.values():
                _number(value, name)
        else:
            raise ValueError(f"elevation.{name} needs offset+blur or the four light/dark fields")


def elevation_fields(entry):
    """Both shadows share one offset and blur unless the entry spells all four out."""
    if "offset" in entry:
        return {
            "lightOffset": entry["offset"],
            "lightBlur": entry["blur"],
            "darkOffset": entry["offset"],
            "darkBlur": entry["blur"],
        }
    return dict(entry)


def swift_number(value):
    return repr(value)


def rust_number(value):
    return repr(float(value))


SWIFT_WIDTH = 100


def swift_wrap(items, indent):
    """`items` joined as swift-format lays out a long argument or element list: as many per line
    as fit in 100 columns, each followed by a comma."""
    lines = []
    line = " " * indent
    for item in items:
        piece = f"{item},"
        if line.strip() and len(line) + 1 + len(piece) > SWIFT_WIDTH:
            lines.append(line)
            line = " " * indent
        line = f"{line} {piece}" if line.strip() else f"{line}{piece}"
    lines.append(line)
    return "\n".join(lines)


def swift_palette(name, palette):
    lip = palette["wellLip"]
    lip_text = "nil" if lip is None else json.dumps(lip)
    colors = swift_wrap([f'{role}: "{palette[role]}"' for role in COLOR_ROLES], 4)
    ansi = swift_wrap([json.dumps(c) for c in palette["ansi"]], 6)
    agents = ", ".join(json.dumps(c) for c in palette["agents"])
    extras = "".join(f', {role}: "{palette[role]}"' for role in FALLBACKS if role in palette)
    return (
        f"  static let {name} = make(\n"
        f"{colors}\n"
        f"    wellLip: {lip_text}, selection: {json.dumps(palette['selection'])},\n"
        f"    presentationLetterbox: {json.dumps(palette['presentationLetterbox'])},\n"
        f"    agents: [{agents}],\n"
        f"    ansi: [\n{ansi}\n    ]{extras})"
    )


def swift_section(title, values):
    lines = [f"  public enum {title} {{"]
    lines += [f"    public static let {key}: CGFloat = {swift_number(v)}" for key, v in values.items()]
    return "\n".join(lines + ["  }"])


def render_swift(document, header):
    palettes = "\n\n".join(swift_palette(n, p) for n, p in document["palettes"].items())
    sections = [swift_section(title, document[key]) for key, title in NUMERIC_SECTIONS.items()]
    fonts = "\n".join(
        f"    public static let {key} = {json.dumps(value)}"
        for key, value in document["fonts"].items()
    )
    elevations = []
    for name, entry in document["elevation"].items():
        fields = elevation_fields(entry)
        args = ", ".join(f"{k}: {swift_number(fields[k])}" for k in ELEVATION_FIELDS)
        line = f"    public static let {name} = Elevation({args})"
        if len(line) > SWIFT_WIDTH:
            line = f"    public static let {name} = Elevation(\n      {args})"
        elevations.append(line)
    return (
        f"{header}\nimport CoreGraphics\n\n"
        f"extension SoyehtThemePalette {{\n{palettes}\n}}\n\n"
        "/// The design tokens of `contracts/design-tokens.json`: the one source of the Mac and the\n"
        "/// desktop clients' radii, spacing, type scale, shadows and layout metrics.\n"
        "public enum DesignTokens {\n"
        + "\n\n".join(sections)
        + "\n\n  public enum Fonts {\n"
        + fonts
        + "\n  }\n\n"
        "  /// An outer shadow pair of a raised surface: light up and left, dark down and right.\n"
        "  public struct Elevation: Sendable {\n"
        + "".join(f"    public let {k}: CGFloat\n" for k in ELEVATION_FIELDS)
        + "\n"
        + "\n".join(elevations)
        + "\n  }\n}\n"
    )


def hex_u32(value):
    return "0x" + value.removeprefix("#")


def rust_palette(name, palette):
    ansi = palette["ansi"]
    fields = [f"{snake(role)}: {hex_u32(palette[role])}," for role in COLOR_ROLES]
    fields += [
        f"selection: {hex_u32(palette['selection'])},",
        f"presentation_letterbox: {hex_u32(palette['presentationLetterbox'])},",
        "well_lip: "
        + ("None" if palette["wellLip"] is None else f"Some({hex_u32(palette['wellLip'])})")
        + ",",
        "agent_plates: &[" + ", ".join(hex_u32(c) for c in palette["agents"]) + "],",
        "ansi: [" + ", ".join(hex_u32(c) for c in ansi) + "],",
    ]
    for role, slot in FALLBACKS.items():
        fields.append(f"{snake(role)}: {hex_u32(palette.get(role, ansi[slot]))},")
    static = snake(name).upper()
    return f"pub(super) static {static}: Palette = Palette {{\n" + "\n".join(fields) + "\n};"


def rust_module(key, values):
    consts = "\n".join(f"pub const {snake(k).upper()}: f32 = {rust_number(v)};" for k, v in values.items())
    return f"pub mod {snake(key)} {{\n{consts}\n}}"


def render_rust(document, header):
    palettes = "\n".join(rust_palette(n, p) for n, p in document["palettes"].items())
    modules = "\n".join(
        rust_module(key, document[key]) for key in (*NUMERIC_SECTIONS, DESKTOP_SECTION)
    )
    fonts = "\n".join(
        f"pub const {snake(k).upper()}: &str = {json.dumps(v)};" for k, v in document["fonts"].items()
    )
    elevations = []
    for name, entry in document["elevation"].items():
        fields = elevation_fields(entry)
        args = ", ".join(f"{snake(k)}: {rust_number(fields[k])}" for k in ELEVATION_FIELDS)
        elevations.append(f"pub const {snake(name).upper()}: Elevation = Elevation {{ {args} }};")
    elevation = (
        "pub mod elevation {\n"
        "/// An outer shadow pair of a raised surface: light up and left, dark down and right.\n"
        "#[derive(Debug, Clone, Copy, PartialEq)]\n"
        "pub struct Elevation {\n"
        + "".join(f"pub {snake(k)}: f32,\n" for k in ELEVATION_FIELDS)
        + "}\n"
        + "\n".join(elevations)
        + "\n}"
    )
    source = (
        f"{header}\nuse super::Palette;\n\n{palettes}\n{modules}\n{elevation}\n"
        f"pub mod fonts {{\n{fonts}\n}}\n"
    )
    return subprocess.run(
        ["rustfmt", "--edition", "2024"], input=source, text=True, capture_output=True, check=True
    ).stdout


def mac_digest(data):
    """The digest the Mac output carries: of the file without its desktop-only last section, so
    that editing a desktop value leaves every Mac file byte for byte as it was."""
    text = data.decode()
    start = text.index(f',\n  "{DESKTOP_SECTION}": {{')
    if not text.endswith("\n  }\n}\n"):
        raise ValueError("the desktop section must close the file")
    return hashlib.sha256((text[:start] + "\n}\n").encode()).hexdigest()


def render_tokens(document, data):
    """The generated files for the token file `data` (bytes) that parsed to `document`."""
    validate_tokens(document)
    swift_header = f"// Generated from {TOKENS_PATH} (sha256 {mac_digest(data)}). Do not edit."
    rust_header = f"// Generated from {TOKENS_PATH} (sha256 {hashlib.sha256(data).hexdigest()}). Do not edit."
    return {
        SWIFT_OUTPUT: render_swift(document, swift_header),
        RUST_OUTPUT: render_rust(document, rust_header),
    }
