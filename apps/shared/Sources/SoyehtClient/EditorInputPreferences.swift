import Observation

@MainActor @Observable
public final class EditorInputPreferences {
  public static let vimEnabledKey = "soyeht.editor.vim.enabled"
  public static let softWrapKey = "soyeht.editor.softWrap"

  public private(set) var vimEnabled: Bool
  /// One choice for every file, like Zed's global `soft_wrap` setting.
  public private(set) var softWrap: Bool
  private var projectPanels: [String: EditorProjectPanelPreference] = [:]
  @ObservationIgnored private let preferences: any ClientPreferenceStore

  public convenience init() {
    self.init(preferences: UserDefaultsClientPreferenceStore())
  }

  public init(preferences: any ClientPreferenceStore) {
    self.preferences = preferences
    vimEnabled = preferences.bool(forKey: Self.vimEnabledKey)
    softWrap = preferences.bool(forKey: Self.softWrapKey)
  }

  public func setVimEnabled(_ enabled: Bool) {
    vimEnabled = enabled
    preferences.set(enabled, forKey: Self.vimEnabledKey)
  }

  public func setSoftWrap(_ enabled: Bool) {
    softWrap = enabled
    preferences.set(enabled, forKey: Self.softWrapKey)
  }

  /// The workspace's project panel choice; `.automatic` until the person toggles it.
  public func projectPanel(for workspaceId: String) -> EditorProjectPanelPreference {
    if let cached = projectPanels[workspaceId] { return cached }
    let key = Self.projectPanelKey(workspaceId)
    guard preferences.bool(forKey: key + ".chosen") else { return .automatic }
    return preferences.bool(forKey: key + ".shown") ? .shown : .hidden
  }

  public func setProjectPanel(_ choice: EditorProjectPanelPreference, for workspaceId: String) {
    projectPanels[workspaceId] = choice
    let key = Self.projectPanelKey(workspaceId)
    preferences.set(choice != .automatic, forKey: key + ".chosen")
    preferences.set(choice == .shown, forKey: key + ".shown")
  }

  static func projectPanelKey(_ workspaceId: String) -> String {
    "soyeht.editor.projectPanel.\(workspaceId)"
  }
}
