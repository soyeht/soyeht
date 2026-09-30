import Foundation

public protocol PhoneWritingLocalStatePreferences: AnyObject {
  func data(forKey key: String) -> Data?
  func set(_ value: Data?, forKey key: String)
}

public final class UserDefaultsPhoneWritingLocalStatePreferences:
  PhoneWritingLocalStatePreferences
{
  private let defaults: UserDefaults

  public init(defaults: UserDefaults = .standard) {
    self.defaults = defaults
  }

  public func data(forKey key: String) -> Data? {
    defaults.data(forKey: key)
  }

  public func set(_ value: Data?, forKey key: String) {
    defaults.set(value, forKey: key)
  }
}

/// Persists only iPhone presentation state. The engine's workspace layout is never read or
/// written by this adapter.
public final class PhoneWritingLocalStateStore {
  public static let keyPrefix = "soyeht.phone.writing.state"

  private let preferences: any PhoneWritingLocalStatePreferences

  public convenience init() {
    self.init(preferences: UserDefaultsPhoneWritingLocalStatePreferences())
  }

  public init(preferences: any PhoneWritingLocalStatePreferences) {
    self.preferences = preferences
  }

  public func load(engineId: String, workspaceId: String) -> PhoneWritingLocalState? {
    guard let data = preferences.data(forKey: key(engineId: engineId, workspaceId: workspaceId))
    else { return nil }
    guard let state = PhoneWritingLocalState.decoded(data) else {
      preferences.set(nil, forKey: key(engineId: engineId, workspaceId: workspaceId))
      return nil
    }
    return state
  }

  public func save(
    _ state: PhoneWritingLocalState, engineId: String, workspaceId: String
  ) throws {
    preferences.set(
      try state.encoded(), forKey: key(engineId: engineId, workspaceId: workspaceId))
  }

  public func remove(engineId: String, workspaceId: String) {
    preferences.set(nil, forKey: key(engineId: engineId, workspaceId: workspaceId))
  }

  public func key(engineId: String, workspaceId: String) -> String {
    "\(Self.keyPrefix).\(engineId).\(workspaceId)"
  }
}
