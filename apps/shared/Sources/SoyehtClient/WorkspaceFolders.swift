import Foundation
import SoyehtProtocol

/// A folder on the Mac that a workspace can be made for.
public struct WorkspaceFolder: Hashable, Identifiable, Sendable {
  public let path: String
  public let name: String
  /// The workspace already rooted here, if any.
  public let workspaceId: String?
  public var id: String { path }

  public init(path: String, workspaceId: String? = nil) {
    self.path = path
    self.name = WorkspaceFolderPolicy.name(of: path)
    self.workspaceId = workspaceId
  }
}

public enum FolderPathProblem: Error, Equatable, Sendable {
  case empty
  case notAbsolute

  public var explanation: String {
    switch self {
    case .empty: "Type the folder's full path on the Mac."
    case .notAbsolute:
      "Use the full path from the top of the disk, such as /Users/you/project. A ~ is not expanded."
    }
  }
}

/// Which folders the paired-device sheets offer, and how a typed path is read. The Mac's engine
/// is the only judge of whether a folder exists; this reads what a person typed into the form
/// the engine accepts, so an obviously wrong entry is refused before the round trip.
public enum WorkspaceFolderPolicy {
  /// The folders the Mac already works in: the roots of its workspaces in their order, then
  /// the folders panes started in that are not a root, each once.
  public static func recent(workspaces: [Workspace], panes: [Pane]) -> [WorkspaceFolder] {
    var seen = Set<String>()
    var folders: [WorkspaceFolder] = []
    for workspace in workspaces where seen.insert(workspace.path).inserted {
      folders.append(WorkspaceFolder(path: workspace.path, workspaceId: workspace.workspaceId))
    }
    for pane in panes where !pane.cwd.isEmpty && seen.insert(pane.cwd).inserted {
      folders.append(WorkspaceFolder(path: pane.cwd))
    }
    return folders
  }

  /// A typed path as the engine takes it: trimmed, absolute, without a trailing separator.
  public static func resolve(_ typed: String) -> Result<String, FolderPathProblem> {
    let path = typed.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !path.isEmpty else { return .failure(.empty) }
    guard isAbsolute(path) else { return .failure(.notAbsolute) }
    var trimmed = path
    while trimmed.count > 1, let last = trimmed.last, last == "/" || last == "\\",
      !trimmed.hasSuffix(":\\"), !trimmed.hasSuffix(":/")
    {
      trimmed.removeLast()
    }
    return .success(trimmed)
  }

  /// The last component of a path, which names a workspace made for it.
  public static func name(of path: String) -> String {
    let parts = path.split(whereSeparator: { $0 == "/" || $0 == "\\" })
    return parts.last.map(String.init) ?? path
  }

  /// The workspace already rooted at `path`, which a new one reuses.
  public static func existingWorkspace(at path: String, in workspaces: [Workspace]) -> Workspace? {
    workspaces.first { $0.path == path }
  }

  private static func isAbsolute(_ path: String) -> Bool {
    if path.hasPrefix("/") || path.hasPrefix("\\\\") { return true }
    let characters = Array(path)
    return characters.count >= 3 && characters[0].isLetter && characters[1] == ":"
      && (characters[2] == "\\" || characters[2] == "/")
  }
}

/// A position inside a workspace's folder, for choosing a sub-folder as a new workspace. The
/// engine lists only inside the workspace roots it already has, so browsing starts at a root.
public struct WorkspaceFolderBrowser: Hashable, Sendable {
  public let root: WorkspaceFolder
  /// Empty at the root; otherwise the folder's path relative to it, separated by `/`.
  public private(set) var relativePath = ""

  public init?(root: WorkspaceFolder) {
    guard root.workspaceId != nil else { return nil }
    self.root = root
  }

  public var workspaceId: String? { root.workspaceId }

  /// The folder shown, as the path the engine takes for a new workspace.
  public var path: String {
    guard !relativePath.isEmpty else { return root.path }
    let separator = root.path.contains("\\") && !root.path.contains("/") ? "\\" : "/"
    let tail = relativePath.replacingOccurrences(of: "/", with: separator)
    return root.path.hasSuffix(separator) ? root.path + tail : root.path + separator + tail
  }

  public var name: String { WorkspaceFolderPolicy.name(of: path) }

  /// The folder inside this one that `entry` names. `entry.path` is relative to the root.
  public func entering(_ entry: ProjectEntry) -> WorkspaceFolderBrowser {
    var next = self
    next.relativePath = entry.path
    return next
  }

  /// The folder above this one, or `nil` at the root.
  public func leaving() -> WorkspaceFolderBrowser? {
    guard !relativePath.isEmpty else { return nil }
    var next = self
    if let slash = relativePath.lastIndex(of: "/") {
      next.relativePath = String(relativePath[..<slash])
    } else {
      next.relativePath = ""
    }
    return next
  }

  /// The folders of a listing in name order, without the ones the project ignores.
  public static func folders(in listing: ProjectTreeListResult) -> [ProjectEntry] {
    listing.entries
      .filter { $0.kind == .directory && !$0.ignored }
      .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
  }
}

/// The text of a workspace's name while it is being changed.
public enum WorkspaceNaming {
  /// The name to save, or `nil` while it is blank or unchanged.
  public static func submission(_ typed: String, current: String) -> String? {
    let name = typed.trimmingCharacters(in: .whitespacesAndNewlines)
    return name.isEmpty || name == current ? nil : name
  }
}

extension EngineClient {
  /// The workspace for a folder on the Mac. A folder that already has one gets that one back;
  /// otherwise the engine makes it, named `name` or after the folder.
  @discardableResult
  public func createWorkspace(path: String, name: String? = nil) async throws -> Workspace {
    if let existing = WorkspaceFolderPolicy.existingWorkspace(at: path, in: workspaces) {
      return existing
    }
    let result = try await mutation(
      .workspaceCreate,
      WorkspaceCreateParams(
        name: name ?? WorkspaceFolderPolicy.name(of: path), path: path,
        idempotencyKey: UUID().uuidString))
    let workspace = try result.decoded(WorkspaceCreateResult.self).workspace
    upsertWorkspace(workspace)
    return workspace
  }

  public func renameWorkspace(_ workspaceId: String, to name: String) async throws {
    let result = try await mutation(
      .workspaceRename,
      WorkspaceRenameParams(
        workspaceId: workspaceId, name: name, idempotencyKey: UUID().uuidString))
    upsertWorkspace(try result.decoded(WorkspaceRenameResult.self).workspace)
  }

  /// The folders directly inside `browser`'s folder, for choosing one as a new workspace.
  public func listFolders(in browser: WorkspaceFolderBrowser) async throws -> [ProjectEntry] {
    guard let workspaceId = browser.workspaceId else { return [] }
    let relative = browser.relativePath
    let listing = try await request(
      .projectTreeList,
      .encoded(ProjectTreeListParams(workspaceId: workspaceId, path: relative))
    ).decoded(ProjectTreeListResult.self)
    return WorkspaceFolderBrowser.folders(in: listing)
  }
}
