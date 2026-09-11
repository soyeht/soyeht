import Foundation

public enum DevProfile {
  public static let dataDirectory = FileManager.default.homeDirectoryForCurrentUser
    .appending(path: "Library/Application Support/Soyeht.next/dev", directoryHint: .isDirectory)
  public static let socketPath: String = {
    #if DEBUG
      if let value = ProcessInfo.processInfo.environment["SOYEHT_TEST_SOCKET"],
        let fixture = fixtureSocket(value)
      {
        return fixture
      }
    #endif
    return dataDirectory.appending(path: "engine.sock").path
  }()
  /// Debug UI tests may use disposable sockets, never another installed profile.
  static func fixtureSocket(_ value: String) -> String? {
    let path = URL(fileURLWithPath: value).resolvingSymlinksInPath().standardizedFileURL
    guard path.lastPathComponent == "engine.sock",
      path.deletingLastPathComponent().deletingLastPathComponent().path
        == URL(fileURLWithPath: "/tmp").resolvingSymlinksInPath().standardizedFileURL.path,
      path.deletingLastPathComponent().lastPathComponent.hasPrefix("sn-")
    else { return nil }
    return path.path
  }
}
