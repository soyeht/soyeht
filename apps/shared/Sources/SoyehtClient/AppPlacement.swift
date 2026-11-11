#if os(macOS)
  import Foundation
  import SoyehtProtocol

  /// Where the released Mac app may run (ADR 0120, "Where the app may run").
  ///
  /// macOS keys the privacy grants of the bundle's helper programs (Files & Folders, Automation,
  /// App Management) by path, and the login agents are registered at the path the app had when
  /// it registered them. An app that is moved later gets new rows in System Settings and leaves
  /// the old ones behind. So the engine is registered only from a stable place, and an app that
  /// runs anywhere else offers to move itself first. This is the one rule; the service setup, the
  /// move offer and the menu all ask it.
  ///
  /// Pure: it reads no file, so the answer for a path is the same whether the path exists. A link
  /// to the copy in Applications is told apart from it by `AppMover`, which compares files.
  public enum AppPlacement {
    public enum Place: Equatable, Sendable {
      /// `/Applications` or `~/Applications`, with any subfolder.
      case stable
      /// A mounted volume (`/Volumes/...`): read-only, and gone when it is ejected.
      case diskImage
      /// An App Translocation path: a random read-only copy of a downloaded app that has not
      /// been moved.
      case translocated
      /// Anywhere else, such as `~/Downloads` or the desktop.
      case elsewhere

      /// Whether the original stays where it is after a move: nothing in a volume or a
      /// translocated path may be changed, and the person ejects the image afterwards.
      public var isReadOnlyOrigin: Bool { self == .diskImage || self == .translocated }
    }

    static let systemFolder = URL(fileURLWithPath: "/Applications", isDirectory: true)
    private static let volumesFolder = "Volumes"
    private static let translocationFolder = "AppTranslocation"

    public static func userFolder(home: URL) -> URL {
      home.appending(path: "Applications", directoryHint: .isDirectory)
    }

    /// The folders the app may be installed to, in the order it tries them: the system one, then
    /// the person's own when the system one cannot be written.
    public static func installFolders(home: URL) -> [URL] {
      [systemFolder, userFolder(home: home)]
    }

    /// The place of `url`, a bundle or any path inside one. Folder names are compared without
    /// regard to case: the system volume is case-insensitive by default, so `/applications/X.app`
    /// is `/Applications/X.app`.
    public static func place(of url: URL, home: URL) -> Place {
      let parts = lowered(url)
      if parts.contains(translocationFolder.lowercased()) { return .translocated }
      if parts.count > 1, parts[1] == volumesFolder.lowercased() { return .diskImage }
      let roots = installFolders(home: home).map { lowered($0) }
      let inside = roots.contains { root in
        parts.count > root.count && Array(parts.prefix(root.count)) == root
      }
      return inside ? .stable : .elsewhere
    }

    private static func lowered(_ url: URL) -> [String] {
      url.standardizedFileURL.pathComponents.map { $0.lowercased() }
    }

    public static func isStable(_ url: URL, home: URL) -> Bool {
      place(of: url, home: home) == .stable
    }

    /// Whether a profile is held to the rule: a released one. The Dev profile installs to its own
    /// folder (`docs/dev-local.md`) and runs from wherever it was built.
    public static func appliesTo(_ profile: ProfileValues) -> Bool {
      profile.name != ProfileValues.dev.name
    }
  }
#endif
