import Observation

@MainActor @Observable
public final class EditorInputPreferences {
  public static let vimEnabledKey = "soyeht.editor.vim.enabled"

  public private(set) var vimEnabled: Bool
  @ObservationIgnored private let preferences: any ClientPreferenceStore

  public convenience init() {
    self.init(preferences: UserDefaultsClientPreferenceStore())
  }

  public init(preferences: any ClientPreferenceStore) {
    self.preferences = preferences
    vimEnabled = preferences.bool(forKey: Self.vimEnabledKey)
  }

  public func setVimEnabled(_ enabled: Bool) {
    vimEnabled = enabled
    preferences.set(enabled, forKey: Self.vimEnabledKey)
  }
}
