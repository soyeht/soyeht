import unittest

from check_mac_text_fonts import violations


class MacTextFontTests(unittest.TestCase):
    def test_rejects_single_and_multiline_system_fonts_on_text(self) -> None:
        source = '''
Text("one").font(.system(size: 12))
Text(
  "two"
)
.font(
  .system(size: 13, weight: .bold)
)
'''
        self.assertEqual(violations(source), [2, 3])

    def test_allows_theme_fonts_and_system_fonts_on_symbols(self) -> None:
        source = '''
Text("copy").font(MacTheme.chrome(12))
Text(code).font(MacTheme.mono(11))
Text("story").font(.custom("Newsreader", size: 18))
let mono = NSFont(name: "JetBrains Mono", size: 12)
Image(systemName: "plus").font(.system(size: 12))
'''
        self.assertEqual(violations(source), [])

    def test_rejects_unapproved_literal_fonts(self) -> None:
        source = '''
Text("copy").font(.custom("Unapproved Serif", size: 18))
let font = NSFont(name: "Other Mono", size: 12)
'''
        self.assertEqual(violations(source), [2, 3])


if __name__ == "__main__":
    unittest.main()
