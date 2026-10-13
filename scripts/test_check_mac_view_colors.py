import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from check_mac_view_colors import scan, theme_catalog_violations, violations


class MacViewColorTests(unittest.TestCase):
    def test_rejects_constructed_and_direct_system_colors(self) -> None:
        source = '''
Text("one").foregroundStyle(Color(red: 1, green: 0, blue: 0))
Circle().fill(
  .red
)
Text("two").foregroundStyle(.secondary)
Text("three").foregroundColor(.red)
Text("four").shadow(color: .blue, radius: 2)
Text("five").foregroundStyle(Color.white)
'''
        self.assertEqual(violations(source), [2, 3, 6, 7, 8, 9])

    def test_allows_theme_clear_strings_and_comments(self) -> None:
        source = '''
Text("Color(red: 1, green: 0, blue: 0)").foregroundStyle(MacTheme.ink)
Color.clear.frame(width: 1)
// Circle().fill(.red)
/* Text("x").foregroundStyle(.secondary) */
Text("x").shadow(color: MacTheme.shadowDark, radius: 2)
'''
        self.assertEqual(violations(source), [])

    def test_scans_mac_and_iphone_sources(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            for relative, source in (
                ("apps/mac/Sources/Mac.swift", "let color = MacTheme.ink\n"),
                ("apps/ios/Sources/Phone.swift", "Text(\"x\").foregroundColor(.red)\n"),
            ):
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")
            self.assertEqual(
                [(path.relative_to(root).as_posix(), line) for path, line in scan(root)],
                [("apps/ios/Sources/Phone.swift", 1)],
            )

    def test_theme_colors_control_rejects_literal_tokens_and_unprojected_palettes(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_theme_colors(root, 'let legacy = "#E0E5EC"')
            problems = theme_catalog_violations(root)
            self.assertIn("ThemeColors must not contain literal hex values", problems)
            self.assertIn("ThemeColors must project every color from the selected palette", problems)

    def test_theme_colors_control_accepts_closed_palette_projection(self) -> None:
        with TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_theme_colors(
                root,
                """
let palette = TerminalColorTheme.seededDefault.palette
let active = SoyehtThemeColorSet(palette: palette)
""",
            )
            self.assertEqual(theme_catalog_violations(root), [])

    @staticmethod
    def write_theme_colors(root: Path, colors: str) -> None:
        path = root / "apps/shared/Sources/SoyehtClient/ThemeColors.swift"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(colors, encoding="utf-8")


if __name__ == "__main__":
    unittest.main()
