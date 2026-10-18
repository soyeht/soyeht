import unittest

from check_mac_environment import ROOT_FILE, violations

ROOT = """
struct App {
  @State private var themeStore = SoyehtThemeStore.shared
  @State private var linkPreferences = WebLinkPreferences()
  var content: some View {
    Window()
      .environment(themeStore)
      .environment(linkPreferences)
  }
}
"""


class MacEnvironmentTests(unittest.TestCase):
    def test_accepts_reads_of_models_injected_at_the_root(self) -> None:
        files = {
            ROOT_FILE: ROOT,
            "apps/mac/Sources/Panel.swift": "@Environment(WebLinkPreferences.self) var p\n"
            "@Environment(\\.dismiss) var d\n",
        }
        self.assertEqual(violations(files), [])

    def test_rejects_a_read_of_a_model_the_root_does_not_inject(self) -> None:
        files = {ROOT_FILE: ROOT, "apps/mac/Sources/Panel.swift": "@Environment(Missing.self) var m"}
        self.assertEqual(len(violations(files)), 1)

    def test_rejects_an_object_injected_below_the_root(self) -> None:
        files = {
            ROOT_FILE: ROOT,
            "apps/mac/Sources/Window.swift": "body.environment(webLinkPreferences)\n"
            ".environment(\\.key, 1)\n",
        }
        found = violations(files)
        self.assertEqual(len(found), 1)
        self.assertIn("Window.swift:1", found[0])


if __name__ == "__main__":
    unittest.main()
