import Foundation
import SoyehtProtocol

/// The installed profile this process runs as, and where that profile keeps its data
/// (`contracts/profiles.json`). The Mac app has only its own profile: the Dev app and the
/// production app are told apart by their bundle identifiers and keep separate folders.
public enum ActiveProfile {
  public static let values = ProfileValues.selected(
    environment: ProcessInfo.processInfo.environment,
    bundleIdentifier: Bundle.main.bundleIdentifier)
  /// The main window's title (`displayNames` of `contracts/profiles.json`).
  public static let windowTitle = values.windowTitle
  public static let dataDirectory = URL.applicationSupportDirectory
    .appending(path: values.applicationSupportSubpath, directoryHint: .isDirectory)
  public static let socketPath: String = {
    #if DEBUG
      if let value = ProcessInfo.processInfo.environment["SOYEHT_TEST_SOCKET"],
        let fixture = fixtureSocket(value)
      {
        return fixture
      }
    #endif
    return dataDirectory.appending(path: ProfileValues.engineSocketName).path
  }()
  /// Debug UI tests may use disposable sockets, never another installed profile.
  static func fixtureSocket(_ value: String) -> String? {
    let path = URL(fileURLWithPath: value).resolvingSymlinksInPath().standardizedFileURL
    guard path.lastPathComponent == ProfileValues.engineSocketName,
      path.deletingLastPathComponent().deletingLastPathComponent().path
        == URL(fileURLWithPath: "/tmp").resolvingSymlinksInPath().standardizedFileURL.path,
      path.deletingLastPathComponent().lastPathComponent.hasPrefix("sn-")
    else { return nil }
    return path.path
  }
}

extension ProfileValues {
  /// The profile a process runs as: the one the environment names, else the one whose Mac app
  /// identifier is this app's bundle identifier, else Dev. A name that no profile has is
  /// ignored, so a stray variable cannot stop the app.
  static func selected(environment: [String: String], bundleIdentifier: String?) -> ProfileValues {
    if let name = environment[selectionEnvironment],
      let named = all.first(where: { $0.name == name })
    {
      return named
    }
    if let bundleIdentifier, let own = all.first(where: { $0.appBundleId == bundleIdentifier }) {
      return own
    }
    return .dev
  }
}

extension ProfileValues {
  /// What a screen says of the profile. A Dev build names itself, so a person who runs it beside
  /// the installed app does not take one for the other; the production build is the product and
  /// says nothing of its profile.
  public var note: String? { name == ProfileValues.dev.name ? "Dev profile" : nil }

  /// The line under Preferences' panels: the product version, then the profile when it has a note.
  public func versionLine(version: String) -> String {
    note.map { "Soyeht \(version) · \($0)" } ?? "Soyeht \(version)"
  }

  /// The connection text of a client that is still connecting.
  public var connectingText: String {
    note.map { "Connecting to the \($0)…" } ?? "Connecting to the engine…"
  }

  /// The connection text of a client that is connected.
  public var connectedText: String {
    note.map { "\($0) · connected" } ?? "Connected"
  }
}
