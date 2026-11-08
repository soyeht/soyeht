"""Renders contracts/design-tokens.json into the Swift, Rust and Kotlin token files.

The palettes and the numeric tokens (radii, spacing, type scale, elevation, layout metrics,
fonts) are defined once in the JSON. The Mac client and the desktop client consume the
generated forms and keep no copy of their own.
"""

import json
import re
import subprocess

from generated_header import header_text

TOKENS_PATH = "contracts/design-tokens.json"
SWIFT_OUTPUT = "apps/shared/Sources/SoyehtClient/Generated.swift"
RUST_OUTPUT = "apps/desktop/crates/soyeht-desktop/src/theme/generated.rs"
KOTLIN_OUTPUT = "apps/android/app/src/main/kotlin/com/soyeht/next/android/theme/DesignTokens.kt"

HEX = re.compile(r"^#[0-9A-F]{6}$")
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
# Roles only the e-ink palette has (ADR 0153): the edge of a card, well, pill or modal, which a screen
# that shows few grey levels needs where the other palettes use a shadow, and the ink of dimmed text,
# which is a lighter step there and never an alpha.
EINK_ROLES = ("border", "dim")
EINK_PALETTE = "einkPalette"
PALETTE_KEYS = set(COLOR_ROLES) | {
    "displayName",
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
    "editor": "Editor",
    "eink": "Eink",
}
# Values only the desktop client has (window frame, popups, glyph sizes). They are Rust-only: the
# Mac output does not mention them, so it does not change when they do.
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


def validate_palette(name, palette, extra=()):
    if not re.fullmatch(r"[a-z][A-Za-z]*", name):
        raise ValueError(f"palette {name!r} is not a lower camel case name")
    unknown = set(palette) - PALETTE_KEYS - set(extra)
    if unknown:
        raise ValueError(f"palette {name}: unknown keys {sorted(unknown)}")
    if not isinstance(palette.get("displayName"), str) or not palette["displayName"]:
        raise ValueError(f"palette {name}: displayName is required")
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
    for role in extra:
        _hex(palette.get(role), f"palette {name} {role}")


def validate_tokens(document):
    expected = {"palettes", EINK_PALETTE, "fonts", "elevation", DESKTOP_SECTION, *NUMERIC_SECTIONS}
    if set(document) != expected:
        raise ValueError(f"design tokens need exactly {sorted(expected)}")
    if list(document)[-1] != DESKTOP_SECTION:
        raise ValueError("the desktop section must be the last one")
    if not document["palettes"]:
        raise ValueError("at least one palette is required")
    for name, palette in document["palettes"].items():
        validate_palette(name, palette)
    validate_palette(EINK_PALETTE, document[EINK_PALETTE], EINK_ROLES)
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


def theme_case(name):
    """`milk` is the theme `neoMilk`: the name persisted in the Mac's settings."""
    return "neo" + name[:1].upper() + name[1:]


def swift_themes(document):
    palettes = document["palettes"]
    cases = "\n".join(f"  case {theme_case(name)}" for name in palettes)
    names = "\n".join(
        f"    case .{theme_case(name)}: {json.dumps(p['displayName'], ensure_ascii=False)}"
        for name, p in palettes.items()
    )
    mapping = "\n".join(f"    case .{theme_case(name)}: .{name}" for name in palettes)
    return (
        "/// The reviewed themes, in the order of `palettes` in the token file. The first one is\n"
        "/// the seeded default.\n"
        "public enum TerminalColorTheme: String, CaseIterable, Codable, Sendable {\n"
        f"{cases}\n\n"
        f"  public static let seededDefault = TerminalColorTheme.{theme_case(next(iter(palettes)))}\n\n"
        f"  public var displayName: String {{\n    switch self {{\n{names}\n    }}\n  }}\n\n"
        f"  public var palette: SoyehtThemePalette {{\n    switch self {{\n{mapping}\n    }}\n  }}\n"
        "}\n"
    )


def rust_themes(document):
    palettes = document["palettes"]
    variants = [theme_case(name)[:1].upper() + theme_case(name)[1:] for name in palettes]
    cases = "\n".join(f"{v}," for v in variants)
    listed = ", ".join(f"Self::{v}" for v in variants)
    palette_arms = "\n".join(
        f"Self::{v} => &{snake(name).upper()}," for v, name in zip(variants, palettes)
    )
    name_arms = "\n".join(
        f"Self::{v} => {json.dumps(p['displayName'], ensure_ascii=False)},"
        for v, p in zip(variants, palettes.values())
    )
    id_arms = "\n".join(
        f"Self::{v} => {json.dumps(theme_case(name))}," for v, name in zip(variants, palettes)
    )
    return (
        "/// The reviewed themes, in the order of `palettes` in the token file. The first one is the\n"
        "/// seeded default.\n"
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n"
        f"pub enum ThemeName {{\n{cases}\n}}\n\n"
        "impl ThemeName {\n"
        f"pub const ALL: &[Self] = &[{listed}];\n"
        "pub const DEFAULT: Self = Self::ALL[0];\n\n"
        f"pub fn palette(self) -> &'static Palette {{\nmatch self {{\n{palette_arms}\n}}\n}}\n\n"
        f"pub fn display_name(self) -> &'static str {{\nmatch self {{\n{name_arms}\n}}\n}}\n\n"
        "/// The name a client persists the choice under: the Mac's `TerminalColorTheme` raw value.\n"
        f"pub fn id(self) -> &'static str {{\nmatch self {{\n{id_arms}\n}}\n}}\n\n"
        "/// The theme persisted as `id`; `None` for a name this build does not know.\n"
        "pub fn from_id(id: &str) -> Option<Self> {\n"
        "Self::ALL.iter().copied().find(|theme| theme.id() == id)\n}\n"
        "}"
    )


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
        f"{swift_themes(document)}\n"
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
        f"{header}\nuse super::Palette;\n\n{rust_themes(document)}\n{palettes}\n{modules}\n{elevation}\n"
        f"pub mod fonts {{\n{fonts}\n}}\n"
    )
    return subprocess.run(
        ["rustfmt", "--edition", "2024"], input=source, text=True, capture_output=True, check=True
    ).stdout


def kotlin_color(value):
    """An opaque ARGB colour as a Kotlin `Long` literal."""
    return "0xFF" + value.removeprefix("#")


def kotlin_palette(name, palette):
    ansi = palette["ansi"]
    fields = [f"{role} = {kotlin_color(palette[role])}," for role in COLOR_ROLES]
    fields += [
        f"selection = {kotlin_color(palette['selection'])},",
        f"presentationLetterbox = {kotlin_color(palette['presentationLetterbox'])},",
        "wellLip = " + ("null" if palette["wellLip"] is None else kotlin_color(palette["wellLip"])) + ",",
        "agents = listOf(" + ", ".join(kotlin_color(c) for c in palette["agents"]) + "),",
        "ansi = listOf(" + ", ".join(kotlin_color(c) for c in ansi) + "),",
    ]
    for role, slot in FALLBACKS.items():
        fields.append(f"{role} = {kotlin_color(palette.get(role, ansi[slot]))},")
    for role in EINK_ROLES:
        fields.append(f"{role} = " + (kotlin_color(palette[role]) if role in palette else "null") + ",")
    body = "\n".join(f"        {field}" for field in fields)
    return f"    val {name} = Palette(\n        displayName = {json.dumps(palette['displayName'], ensure_ascii=False)},\n{body}\n    )"


def kotlin_section(title, values):
    consts = "\n".join(f"    const val {key}: Float = {float(v)!r}f" for key, v in values.items())
    return f"object {title} {{\n{consts}\n}}"


def render_kotlin(document, header):
    """The palettes and numeric tokens the Android app draws with (ADR 0134). Colours are ARGB
    `Long`s so that this file needs no Android library; the theme turns them into `Color`."""
    roles = (*COLOR_ROLES, "selection", "presentationLetterbox")
    fields = "".join(f"    val {role}: Long,\n" for role in roles)
    palettes = "\n\n".join(kotlin_palette(n, p) for n, p in document["palettes"].items())
    eink = kotlin_palette("eink", document[EINK_PALETTE])
    listed = ", ".join(f"{name!r}" .replace("'", '"') + " to " + name for name in document["palettes"])
    sections = "\n\n".join(kotlin_section(title, document[key]) for key, title in NUMERIC_SECTIONS.items())
    fonts = "\n".join(f"    const val {key}: String = {json.dumps(value)}" for key, value in document["fonts"].items())
    return (
        f"{header}\npackage com.soyeht.next.android.theme\n\n"
        "/** One theme: the palette roles every view draws with. */\n"
        "class Palette(\n    val displayName: String,\n"
        f"{fields}"
        "    val wellLip: Long?,\n    val agents: List<Long>,\n    val ansi: List<Long>,\n"
        "    val syntaxType: Long,\n    val diffAdded: Long,\n"
        "    /** The edge of a card, well, pill or modal, and the ink of dimmed text: the e-ink palette's own. */\n"
        "    val border: Long?,\n    val dim: Long?,\n)\n\n"
        f"object Palettes {{\n{palettes}\n\n"
        "    /** The palette of the e-ink mode: not a theme the person picks, so it is not in [all]. */\n"
        f"{eink}\n\n    /** The themes by name, in the order of the token file; the first is the default. */\n"
        f"    val all: List<Pair<String, Palette>> = listOf({listed})\n}}\n\n"
        f"{sections}\n\nobject Fonts {{\n{fonts}\n}}\n"
    )


def render_tokens(document):
    """The generated files for the parsed token file `document`."""
    validate_tokens(document)
    header = f"// {header_text(TOKENS_PATH)}"
    return {
        SWIFT_OUTPUT: render_swift(document, header),
        RUST_OUTPUT: render_rust(document, header),
        KOTLIN_OUTPUT: render_kotlin(document, header),
    }
