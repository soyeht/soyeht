import Testing

@testable import SoyehtClient

private final class MemoryThemePreferenceStore: ThemePreferenceStore {
  private var values: [String: String]

  init(values: [String: String] = [:]) {
    self.values = values
  }

  func string(forKey key: String) -> String? {
    values[key]
  }

  func set(_ value: String, forKey key: String) {
    values[key] = value
  }
}

@Test
@MainActor
func themeStoreSeedsMilkAndRejectsUnknownPersistence() {
  let empty = SoyehtThemeStore(preferences: MemoryThemePreferenceStore())
  let invalid = SoyehtThemeStore(
    preferences: MemoryThemePreferenceStore(
      values: [SoyehtThemeStore.activeThemeKey: "unknown-theme"]))

  #expect(empty.activeTheme == .neoMilk)
  #expect(invalid.activeTheme == .neoMilk)
}

@Test
@MainActor
func themeSelectionIsImmediateAndSurvivesStoreRestart() {
  let preferences = MemoryThemePreferenceStore()
  let live = SoyehtThemeStore(preferences: preferences)

  live.select(.neoDeepForest)
  #expect(live.activeTheme == .neoDeepForest)
  #expect(live.activeTheme.palette.isDark)

  let restored = SoyehtThemeStore(preferences: preferences)
  #expect(restored.activeTheme == .neoDeepForest)
}

@Test
@MainActor
func everyClosedThemeCanBecomeActive() {
  let preferences = MemoryThemePreferenceStore()
  let store = SoyehtThemeStore(preferences: preferences)

  for theme in TerminalColorTheme.allCases {
    store.select(theme)
    #expect(store.activeTheme == theme)
    #expect(store.activeTheme.palette == theme.palette)
  }
}
