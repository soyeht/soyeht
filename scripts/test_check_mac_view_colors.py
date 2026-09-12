import unittest

from check_mac_view_colors import violations


class MacViewColorTests(unittest.TestCase):
    def test_rejects_constructed_and_direct_system_colors(self) -> None:
        source = '''
Text("one").foregroundStyle(Color(red: 1, green: 0, blue: 0))
Circle().fill(
  .red
)
Text("two").foregroundStyle(.secondary)
'''
        self.assertEqual(violations(source), [2, 3, 6])

    def test_allows_theme_clear_strings_and_comments(self) -> None:
        source = '''
Text("Color(red: 1, green: 0, blue: 0)").foregroundStyle(MacTheme.ink)
Color.clear.frame(width: 1)
// Circle().fill(.red)
/* Text("x").foregroundStyle(.secondary) */
'''
        self.assertEqual(violations(source), [])


if __name__ == "__main__":
    unittest.main()
