import copy
import json
import re
import subprocess
import unittest
from pathlib import Path

from design_tokens import (
    DESKTOP_SECTION,
    FALLBACKS,
    NUMERIC_SECTIONS,
    RUST_OUTPUT,
    SWIFT_OUTPUT,
    render_tokens,
    validate_tokens,
)

ROOT = Path(__file__).resolve().parent.parent
# The last commit before the tokens were generated: the Mac values it holds are the contract.
BASE = "0d33cdd483697fbc35495221023886dd34c90355"
SHARED = "apps/shared/Sources/SoyehtClient/"
MAC = "apps/mac/Sources/"


TOKEN_BYTES = (ROOT / "contracts/design-tokens.json").read_bytes()


def tokens():
    return json.loads(TOKEN_BYTES)


def git(*arguments):
    return subprocess.run(["git", *arguments], cwd=ROOT, text=True, capture_output=True, check=False)


def ensure_base():
    """The pre-generation commit must be in the clone: fetch it when it is not, never skip."""
    if git("cat-file", "-e", f"{BASE}^{{commit}}").returncode == 0:
        return
    fetched = git("fetch", "--quiet", "--depth=1", "origin", BASE)
    if fetched.returncode != 0 or git("cat-file", "-e", f"{BASE}^{{commit}}").returncode != 0:
        raise AssertionError(
            f"commit {BASE} is not in this clone and could not be fetched: {fetched.stderr.strip()}"
        )


def base_file(path):
    ensure_base()
    result = git("show", f"{BASE}:{path}")
    if result.returncode != 0:
        raise AssertionError(f"{path} is not in commit {BASE}: {result.stderr.strip()}")
    return result.stdout


def squeezed(text):
    return re.sub(r"\s+", " ", text)


def call_arguments(source, start):
    """The text between the parentheses of the call that opens at `start`."""
    depth = 0
    for index in range(start, len(source)):
        if source[index] == "(":
            depth += 1
        elif source[index] == ")":
            depth -= 1
            if depth == 0:
                return source[start + 1 : index]
    raise ValueError("unbalanced call")


def swift_palettes(source):
    """Every `static let name = make(...)` palette of a Swift source, with `make`'s fallbacks."""
    found = {}
    for match in re.finditer(r"static let (\w+) = make\(", source):
        body = call_arguments(source, match.end() - 1)
        palette = {}
        for label in (*PALETTE_ROLES, "wellLip", "syntaxType", "diffAdded"):
            value = re.search(rf'\b{label}: (?:"(#[0-9A-F]{{6}})"|(nil))', body)
            if value:
                palette[label] = value.group(1)
        for label in ("agents", "ansi"):
            colors = re.search(rf"\b{label}: \[(.*?)\]", body, re.S).group(1)
            palette[label] = re.findall(r'"(#[0-9A-F]{6})"', colors)
        palette.setdefault("wellLip", None)
        for role, slot in FALLBACKS.items():
            palette.setdefault(role, palette["ansi"][slot])
        found[match.group(1)] = palette
    return found


PALETTE_ROLES = (
    "screen canvas face hover well ink secondary muted accent onAccent shadowDark shadowLight "
    "wellShadow wellRim selection presentationLetterbox"
).split()


def resolved(palette):
    ansi = palette["ansi"]
    colors = {key: value for key, value in palette.items() if key != "displayName"}
    return colors | {role: palette.get(role, ansi[slot]) for role, slot in FALLBACKS.items()}


def enum_body(source, header):
    start = source.index(header)
    open_at = source.index("{", start)
    depth = 0
    for index in range(open_at, len(source)):
        depth += {"{": 1, "}": -1}.get(source[index], 0)
        if depth == 0:
            return source[open_at + 1 : index]
    raise ValueError(header)


def static_numbers(source, header):
    """`static let name[: Type] = number` lines of the enum that `header` opens."""
    body = enum_body(source, header)
    return {
        name: float(value)
        for name, value in re.findall(
            r"static let (\w+)(?:: \w+)? = ([0-9][0-9.]*)\s*(?:$|//)", body, re.M
        )
    }


class TokenGenerationTests(unittest.TestCase):
    def test_both_outputs_render_from_one_source(self):
        outputs = render_tokens(tokens(), TOKEN_BYTES)
        self.assertEqual(set(outputs), {SWIFT_OUTPUT, RUST_OUTPUT})
        for text in outputs.values():
            self.assertTrue(text.startswith("// Generated from contracts/design-tokens.json"))

    def test_generated_swift_palettes_read_back_as_the_json(self):
        swift = render_tokens(tokens(), TOKEN_BYTES)[SWIFT_OUTPUT]
        parsed = swift_palettes(swift)
        self.assertEqual(tuple(parsed), tuple(tokens()["palettes"]))
        for name, palette in tokens()["palettes"].items():
            self.assertEqual(parsed[name], resolved(palette), name)

    def test_generated_rust_carries_every_palette_color(self):
        rust = render_tokens(tokens(), TOKEN_BYTES)[RUST_OUTPUT]
        for name, palette in tokens()["palettes"].items():
            block = rust.split(f"static {re.sub(r'(?<=[a-z])(?=[A-Z])', '_', name).upper()}:")[1]
            block = block.split("};")[0]
            colors = re.findall(r"0x[0-9A-F]{6}", block)
            expected = [
                "0x" + value.removeprefix("#")
                for value in (
                    *(palette[r] for r in PALETTE_ROLES),
                    *([palette["wellLip"]] if palette["wellLip"] else []),
                    *palette["agents"],
                    *palette["ansi"],
                    resolved(palette)["syntaxType"],
                    resolved(palette)["diffAdded"],
                )
            ]
            self.assertCountEqual(colors, expected, name)

    def test_desktop_values_are_rust_only_and_never_change_the_mac_output(self):
        document = tokens()
        outputs = render_tokens(document, TOKEN_BYTES)
        self.assertIn("pub mod desktop", outputs[RUST_OUTPUT])
        self.assertNotIn("windowWidth", outputs[SWIFT_OUTPUT])
        changed = copy.deepcopy(document)
        changed[DESKTOP_SECTION]["windowWidth"] += 1
        changed_bytes = TOKEN_BYTES.replace(b'"windowWidth": 1280', b'"windowWidth": 1281')
        self.assertNotEqual(changed_bytes, TOKEN_BYTES)
        again = render_tokens(changed, changed_bytes)
        self.assertEqual(again[SWIFT_OUTPUT], outputs[SWIFT_OUTPUT])
        self.assertNotEqual(again[RUST_OUTPUT], outputs[RUST_OUTPUT])

    def test_generated_swift_fits_the_formatter_line_length(self):
        """swift-format lints the generated file: no line past 100 columns but the header."""
        swift = render_tokens(tokens(), TOKEN_BYTES)[SWIFT_OUTPUT]
        for number, line in enumerate(swift.splitlines()[1:], start=2):
            self.assertLessEqual(len(line), 100, f"line {number}")

    def test_the_desktop_section_must_come_last(self):
        document = tokens()
        reordered = {key: document[key] for key in document if key != DESKTOP_SECTION}
        reordered[DESKTOP_SECTION] = document[DESKTOP_SECTION]
        validate_tokens(reordered)
        first = {DESKTOP_SECTION: document[DESKTOP_SECTION]} | {
            key: value for key, value in document.items() if key != DESKTOP_SECTION
        }
        with self.assertRaises(ValueError):
            validate_tokens(first)

    def test_invalid_tokens_fail_closed(self):
        validate_tokens(tokens())
        cases = []
        bad_hex = copy.deepcopy(tokens())
        bad_hex["palettes"]["milk"]["ink"] = "#3e4a66"
        cases.append(bad_hex)
        short_ansi = copy.deepcopy(tokens())
        short_ansi["palettes"]["milk"]["ansi"].pop()
        cases.append(short_ansi)
        nameless = copy.deepcopy(tokens())
        del nameless["palettes"]["milk"]["displayName"]
        cases.append(nameless)
        badly_named = copy.deepcopy(tokens())
        badly_named["palettes"] = {"Milk Tea": badly_named["palettes"]["milk"]}
        cases.append(badly_named)
        unknown = copy.deepcopy(tokens())
        unknown["palettes"]["milk"]["glow"] = "#FFFFFF"
        cases.append(unknown)
        extra = copy.deepcopy(tokens())
        extra["mystery"] = {}
        cases.append(extra)
        text = copy.deepcopy(tokens())
        text["radius"]["pill"] = "round"
        cases.append(text)
        for case in cases:
            with self.assertRaises(ValueError):
                validate_tokens(case)


class MacValuesAreUnchangedTests(unittest.TestCase):
    """The tokens are the values the Mac client had before they were generated."""

    def test_palettes_equal_the_pre_generation_swift_catalogue(self):
        old = swift_palettes(base_file(SHARED + "ThemePalette.swift"))
        self.assertEqual(tuple(old), tuple(tokens()["palettes"]))
        for name, palette in tokens()["palettes"].items():
            self.assertEqual(resolved(palette), old[name], name)

    def test_theme_names_and_order_equal_the_pre_generation_catalogue(self):
        """The raw values are persisted: a rename would lose a person's chosen theme."""
        old = base_file(SHARED + "ThemePalette.swift")
        enum = enum_body(old, "enum TerminalColorTheme")
        cases = re.findall(r"^\s*case (neo\w+)$", enum, re.M)
        names = dict(re.findall(r'case \.(neo\w+): "([^"]+)"', enum))
        document = tokens()["palettes"]
        self.assertEqual(cases, ["neo" + n[:1].upper() + n[1:] for n in document])
        for palette_name, palette in document.items():
            theme = "neo" + palette_name[:1].upper() + palette_name[1:]
            self.assertEqual(names[theme], palette["displayName"], theme)

    def test_numeric_tokens_equal_the_pre_generation_swift_literals(self):
        document = tokens()
        for path, header, section, mapping in NUMERIC_SOURCES:
            numbers = static_numbers(base_file(path), header)
            names = mapping if mapping is not None else {name: name for name in numbers}
            for name, key in names.items():
                with self.subTest(path=path, name=name):
                    self.assertEqual(float(document[section][key]), numbers[name])
            if mapping is None:
                self.assertEqual(set(document[section]) - set(numbers), set(), section)

    def test_inline_metrics_equal_the_old_literals(self):
        document = tokens()
        for path, template, section, key in INLINE_LITERALS:
            literal = squeezed(template.format(document[section][key]))
            self.assertIn(literal, squeezed(base_file(path)), (section, key))

    def test_every_numeric_token_is_proved_against_an_old_mac_value(self):
        document = tokens()
        proved = {(s, k) for _, _, s, m in NUMERIC_SOURCES for k in (m or {}).values()}
        proved |= {(s, k) for _, _, s, m in NUMERIC_SOURCES if m is None for k in document[s]}
        proved |= {(s, k) for _, _, s, k in INLINE_LITERALS}
        every = {(s, k) for s in NUMERIC_SECTIONS for k in document[s]}
        self.assertEqual(every - proved, set())

    def test_elevations_and_fonts_equal_the_old_literals(self):
        document = tokens()
        style = base_file(MAC + "MacStyle.swift")
        for name, entry in document["elevation"].items():
            if "offset" in entry:
                literal = f"static let {name} = Elevation(offset: {entry['offset']}, blur: {entry['blur']})"
            else:
                literal = (
                    f"static let {name} = Elevation(lightOffset: {entry['lightOffset']}, "
                    f"lightBlur: {entry['lightBlur']}, darkOffset: {entry['darkOffset']}, "
                    f"darkBlur: {entry['darkBlur']})"
                )
            self.assertIn(literal, style, name)
        self.assertIn(f'chromeFamily = "{document["fonts"]["chrome"]}"', style)
        self.assertIn(f'screenplayFamily = "{document["fonts"]["screenplay"]}"', style)
        terminal = base_file(SHARED + "MacTerminal.swift")
        self.assertIn(f'uiFamily = "{document["fonts"]["ui"]}"', terminal)
        self.assertIn(f'monoFamily = "{document["fonts"]["mono"]}"', terminal)


def identity(*names):
    return {name: name for name in names}


# (source file, enum header, token section, {Swift name: JSON key}); a mapping of None means
# every number of the enum is a token of that section with the same name.
NUMERIC_SOURCES = [
    (MAC + "MacStyle.swift", "enum Radius", "radius", None),
    (MAC + "MacStyle.swift", "enum Space", "space", None),
    (MAC + "MacStyle.swift", "enum TypeScale", "typeScale", None),
    (SHARED + "PaneShell.swift", "enum PaneShellMetrics", "paneShell", None),
    (SHARED + "PaneInteriorControls.swift", "enum PaneInteriorMetrics", "paneInterior", None),
    (SHARED + "AgentComposerControlViews.swift", "enum AgentComposerMetrics", "composer", None),
    (
        SHARED + "PaneLayoutSpacing.swift",
        "enum PaneLayoutSpacing",
        "paneLayout",
        identity("paneGap", "screenPadding", "gridPadding"),
    ),
    (
        SHARED + "PaneLayoutFloors.swift",
        "enum PaneLayoutContentMetrics",
        "paneLayout",
        identity("paneCardInset", "paneStackSpacing")
        | {
            "genericPaneHeaderHeight": "headerHeight",
            "genericPaneHeaderStateDotWidth": "headerStateDot",
            "genericPaneHeaderActionWidth": "headerActionWidth",
        },
    ),
    (
        MAC + "MacPaneLayoutMetrics.swift",
        "enum MacPaneLayoutMetrics",
        "layout",
        identity(
            "topBarHeight",
            "workspaceTabHeight",
            "topBarControlHeight",
            "workspaceTabMaximumWidth",
            "workspaceOverflowButtonWidth",
            "workspaceOverflowPopoverWidth",
            "workspaceOverflowPopoverMaximumHeight",
            "railWidth",
            "railButtonSize",
        ),
    ),
    (
        MAC + "WorkspaceTabBar.swift",
        "enum WorkspaceTabBarGlyph",
        "glyph",
        identity("tabClose", "chevron") | {"toggle": "topBarToggle", "newPane": "topBarNewPane"},
    ),
]

# Metrics the views spelled inline: the old text with the token's number in place.
INLINE_LITERALS = [
    (MAC + "WorkspaceTabBar.swift", "Color.clear.frame(width: {}, height:", "layout", "topBarLeadingInset"),
    (MAC + "AppRail.swift", ".frame(width: {}, height: 1)", "layout", "railDividerWidth"),
    (MAC + "AppRail.swift", ".font(.system(size: {}, weight: .medium))", "glyph", "rail"),
    (MAC + "UsageViews.swift", "Circle().stroke(MacTheme.well, lineWidth: {})", "usageRing", "trackWidth"),
    (
        MAC + "UsageViews.swift",
        "Circle().stroke(MacTheme.well, lineWidth: 5) }} .frame(width: {0}, height: {0})",
        "usageRing",
        "faceDiameter",
    ),
    (MAC + "UsageViews.swift", "UsageRingArc(presentation, lineWidth: {})", "usageRing", "arcWidth"),
    (
        MAC + "UsageViews.swift",
        "UsageRingArc(presentation, lineWidth: 3) .frame(width: {0}, height: {0})",
        "usageRing",
        "diameter",
    ),
    (
        MAC + "UsageViews.swift",
        "ProviderGlyph(reading.provider) .foregroundStyle(MacTheme.ink) .frame(width: {0}, height: {0})",
        "usageRing",
        "glyphSize",
    ),
    (
        MAC + "UsageViews.swift",
        "}} .frame(width: {0}, height: {0}) Text(presentation.percentage)",
        "usageRing",
        "diameter",
    ),
    (
        MAC + "UsageViews.swift",
        ".opacity(presentation.emphasis.ringOpacity) .frame(width: 40, height: {})",
        "usageRing",
        "height",
    ),
    (MAC + "UsageViews.swift", "MacTheme.chrome({}, weight: .bold)", "usageRing", "percentageSize"),
    (MAC + "UsageViews.swift", ".frame(height: {})", "usageRing", "percentageHeight"),
]


if __name__ == "__main__":
    unittest.main()
