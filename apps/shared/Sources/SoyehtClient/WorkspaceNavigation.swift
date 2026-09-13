import Foundation
import Observation

public enum WorkspaceDestination: String, CaseIterable, Sendable {
  case agents
  case editor
  case web
  case tasks
  case terminal
  case phone
}

public protocol ClientPreferenceStore: AnyObject {
  func bool(forKey key: String) -> Bool
  func set(_ value: Bool, forKey key: String)
}

public final class UserDefaultsClientPreferenceStore: ClientPreferenceStore {
  private let defaults: UserDefaults

  public init(defaults: UserDefaults = .standard) {
    self.defaults = defaults
  }

  public func bool(forKey key: String) -> Bool {
    defaults.bool(forKey: key)
  }

  public func set(_ value: Bool, forKey key: String) {
    defaults.set(value, forKey: key)
  }
}

@MainActor @Observable
public final class WorkspaceNavigation {
  public static let appBarOpenKey = "soyeht.appBar.isOpen"

  public private(set) var destination: WorkspaceDestination
  public private(set) var appBarOpen: Bool
  @ObservationIgnored private let preferences: any ClientPreferenceStore

  public convenience init() {
    self.init(preferences: UserDefaultsClientPreferenceStore())
  }

  public init(preferences: any ClientPreferenceStore) {
    self.preferences = preferences
    destination = .agents
    appBarOpen = preferences.bool(forKey: Self.appBarOpenKey)
  }

  public func select(_ destination: WorkspaceDestination) {
    self.destination = destination
  }

  public func toggleAppBar() {
    appBarOpen.toggle()
    preferences.set(appBarOpen, forKey: Self.appBarOpenKey)
  }
}
