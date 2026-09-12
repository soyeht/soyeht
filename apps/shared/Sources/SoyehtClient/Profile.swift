import Foundation
import SoyehtProtocol

public enum DevProfile {
  public static let dataDirectory = URL.applicationSupportDirectory
    .appending(path: DevProfileValues.applicationSupportSubpath, directoryHint: .isDirectory)
  public static let socketPath: String = {
    #if DEBUG
      if let value = ProcessInfo.processInfo.environment["SOYEHT_TEST_SOCKET"],
        let fixture = fixtureSocket(value)
      {
        return fixture
      }
    #endif
    return dataDirectory.appending(path: DevProfileValues.engineSocketName).path
  }()
  /// Debug UI tests may use disposable sockets, never another installed profile.
  static func fixtureSocket(_ value: String) -> String? {
    let path = URL(fileURLWithPath: value).resolvingSymlinksInPath().standardizedFileURL
    guard path.lastPathComponent == DevProfileValues.engineSocketName,
      path.deletingLastPathComponent().deletingLastPathComponent().path
        == URL(fileURLWithPath: "/tmp").resolvingSymlinksInPath().standardizedFileURL.path,
      path.deletingLastPathComponent().lastPathComponent.hasPrefix("sn-")
    else { return nil }
    return path.path
  }
}
