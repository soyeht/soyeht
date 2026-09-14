import Foundation
import Observation

public protocol ThemePreferenceStore: AnyObject {
  func string(forKey key: String) -> String?
  func set(_ value: String, forKey key: String)
}

public final class UserDefaultsThemePreferenceStore: ThemePreferenceStore {
  private let defaults: UserDefaults

  public init(defaults: UserDefaults = .standard) {
    self.defaults = defaults
  }

  public func string(forKey key: String) -> String? {
    defaults.string(forKey: key)
  }

  public func set(_ value: String, forKey key: String) {
    defaults.set(value, forKey: key)
  }
}

@MainActor @Observable
public final class SoyehtThemeStore {
  public static let activeThemeKey = "soyeht.appearance.theme"
  public static let shared = SoyehtThemeStore()

  public private(set) var activeTheme: TerminalColorTheme
  @ObservationIgnored private let preferences: any ThemePreferenceStore

  public convenience init() {
    self.init(preferences: UserDefaultsThemePreferenceStore())
  }

  public init(preferences: any ThemePreferenceStore) {
    self.preferences = preferences
    activeTheme =
      preferences.string(forKey: Self.activeThemeKey)
      .flatMap(TerminalColorTheme.init(rawValue:)) ?? .seededDefault
  }

  public func select(_ theme: TerminalColorTheme) {
    guard activeTheme != theme else { return }
    activeTheme = theme
    preferences.set(theme.rawValue, forKey: Self.activeThemeKey)
  }
}
