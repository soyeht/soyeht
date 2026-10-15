"""Regression tests for the desktop design-token check."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from check_desktop_tokens import DESKTOP_SRC, problems


def messages(source: str, name: str = "views/card.rs") -> list[str]:
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        file = root / DESKTOP_SRC / name
        file.parent.mkdir(parents=True)
        file.write_text(source)
        return problems(root)


class DesktopTokenTests(unittest.TestCase):
    def test_a_number_added_to_or_taken_from_a_token_in_px_is_listed(self):
        for expression in (
            "px(type_scale::BODY - 1.0)",
            "px(desktop::CHEVRON_GLYPH + 1.0)",
            "px(pane_interior::STACK_SPACING - 2)",
        ):
            found = messages(f"fn a() {{ let _ = {expression}; }}\n")
            self.assertEqual(len(found), 1, expression)

    def test_a_const_derived_from_a_token_is_listed(self):
        found = messages("pub const CARD_GLYPH: f32 = desktop::INLINE_GLYPH - 2.0;\n")
        self.assertEqual(len(found), 1)

    def test_centering_counting_and_space_units_are_allowed(self):
        source = (
            "fn a() {\n"
            "    px((desktop::SWATCH_HEIGHT - desktop::SWATCH_PLATE_HEIGHT) / 2.0);\n"
            "    px(px_count * 2.0 * pane_interior::COMPACT_SPACING);\n"
            "    px(2.0 * pane_interior::COMPACT_SPACING);\n"
            "    px(space::X8 + space::X18);\n"
            "    px(space::X18 * 2.0 - space::X2);\n"
            "    px(type_scale::BODY);\n"
            "    px(layout::TOP_BAR_HEIGHT + 2.0 * clearance);\n"
            "}\n"
        )
        self.assertEqual(messages(source), [])

    def test_a_number_as_the_radius_of_a_surface_is_listed(self):
        self.assertEqual(len(messages("fn a() { neo::raised(palette, 14.0, e); }\n")), 1)
        self.assertEqual(len(messages("fn a() { neo::well(palette, 6, e); }\n")), 1)
        self.assertEqual(messages("fn a() { neo::raised(palette, radius::CARD, e); }\n"), [])

    def test_the_theme_and_test_code_are_exempt(self):
        self.assertEqual(
            messages("pub const A: f32 = desktop::B - 1.0;\n", "theme/mod.rs"), []
        )
        source = "#[cfg(test)]\nmod tests {\n    fn t() { px(type_scale::BODY - 1.0); }\n}\n"
        self.assertEqual(messages(source), [])

    def test_a_comment_or_a_string_naming_the_pattern_is_not_listed(self):
        source = '// px(type_scale::BODY - 1.0)\nconst S: &str = "px(type_scale::BODY - 1.0)";\n'
        self.assertEqual(messages(source), [])


if __name__ == "__main__":
    unittest.main()
