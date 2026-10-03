from __future__ import annotations

import pathlib
import unittest

from check_pane_interior import INTERIOR_FILES, INTERIOR_TYPES, scan, type_spans, violations
from check_mac_view_colors import code_only

ROOT = pathlib.Path(__file__).resolve().parents[1]


class PaneInteriorGateTests(unittest.TestCase):
    def test_rejects_neumorphic_modifiers_and_shadows(self) -> None:
        source = "\n".join(
            [
                "let a = view.neoWell(radius: 3)",
                "let b = view.neoRaised(radius: 4, elevation: .chip)",
                "let c = view.neoAccent()",
                "let d = Button().buttonStyle(NeoPillButtonStyle())",
                "let e = view.shadow(color: style.shadowDark, radius: 2)",
                "let f = style.well.shadow(.inner(color: style.wellShadow, radius: 3))",
            ]
        )
        self.assertEqual([line for line, _ in violations(source)], [1, 2, 3, 4, 5, 6])

    def test_rejects_colors_outside_the_palette(self) -> None:
        source = "\n".join(
            [
                "let a = Color.white",
                "let b = view.foregroundStyle(.white)",
                "let c = Color(red: 1, green: 1, blue: 1)",
                'let d = palette("#FFFFFF")',
                'let e = palette("#A6B4C880")',
                "let f = NSColor.black",
                "let g = view.background(style.canvas)",
            ]
        )
        self.assertEqual([line for line, _ in violations(source)], [1, 2, 3, 4, 5, 6])

    def test_rejects_the_window_chrome_font(self) -> None:
        source = "\n".join(
            [
                "let a = Text(x).font(MacTheme.chrome(MacTheme.TypeScale.small))",
                "let b = Text(x).font(MacTheme.ui(MacTheme.TypeScale.small))",
            ]
        )
        self.assertEqual([line for line, _ in violations(source)], [1])

    def test_ignores_comments_and_ordinary_strings(self) -> None:
        source = "\n".join(
            [
                "// Was .neoWell(radius: 3) with Color.white and \"#FFFFFF\".",
                "/* .shadow(color: x) */",
                'let label = "decade"',
                'let path = "Color.white.shadow("',
            ]
        )
        self.assertEqual(violations(source), [])

    def test_mixed_files_are_checked_only_inside_pane_types(self) -> None:
        source = "\n".join(
            [
                "struct TopBar: View { var body: some View { x.neoRaised() } }",
                "private struct PaneCard: View {",
                "  var body: some View { x.neoWell(radius: 2) }",
                "}",
            ]
        )
        spans = type_spans(code_only(source), ("PaneCard",))
        self.assertEqual([line for line, _ in violations(source, spans)], [3])

    def test_every_guarded_source_exists_and_is_flat(self) -> None:
        for relative in (*INTERIOR_FILES, *INTERIOR_TYPES):
            self.assertTrue((ROOT / relative).is_file(), relative)
        self.assertEqual(scan(ROOT), [])


if __name__ == "__main__":
    unittest.main()
