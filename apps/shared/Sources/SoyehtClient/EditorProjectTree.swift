import SoyehtProtocol

/// The project panel tree as a value: the folders the engine has listed, which
/// ones are expanded, and the selected entry. It holds no I/O, so the Mac and
/// the iPad share its rules and tests cover them without an engine.
public struct EditorProjectTree: Equatable, Sendable {
  public struct Node: Equatable, Sendable {
    public let name: String
    public let path: String
    public let isDirectory: Bool
    public let ignored: Bool
  }

  public struct Row: Identifiable, Equatable, Sendable {
    public let node: Node
    public let depth: Int
    public let expanded: Bool
    public var id: String { node.path }
  }

  /// Keys the tree answers the way Zed's project panel does.
  public enum Key: Sendable {
    case up, down, left, right, first, last
  }

  /// The empty path names the workspace root.
  public static let root = ""

  public private(set) var children: [String: [Node]] = [:]
  public private(set) var expanded: Set<String> = [root]
  public private(set) var truncated: Set<String> = []
  public var selection: String?
  public private(set) var showIgnored = false

  public init() {}

  /// Visible rows in display order: each expanded, listed folder's children
  /// follow it, one level deeper.
  public var rows: [Row] {
    var rows: [Row] = []
    func visit(_ directory: String, depth: Int) {
      for node in children[directory] ?? [] {
        let open = node.isDirectory && expanded.contains(node.path)
        rows.append(Row(node: node, depth: depth, expanded: open))
        if open { visit(node.path, depth: depth + 1) }
      }
    }
    visit(Self.root, depth: 0)
    return rows
  }

  /// Folders whose listing is on screen; the client watches exactly these.
  public var watchedDirectories: [String] {
    expanded.filter { children[$0] != nil && isReachable($0) }.sorted()
  }

  /// Expanded folders on screen that still need a listing; a nested folder
  /// waits until its parent's listing shows it.
  public var directoriesToLoad: [String] {
    expanded.filter { directory in
      children[directory] == nil && isReachable(directory)
        && (directory == Self.root || node(at: directory)?.isDirectory == true)
    }.sorted()
  }

  public func node(at path: String) -> Node? {
    children[Self.parent(of: path)]?.first { $0.path == path }
  }

  /// Stores one folder listing. Entries that disappeared take their expanded
  /// descendants and the selection with them.
  public mutating func apply(_ listing: ProjectTreeListResult) {
    let nodes = listing.entries.map {
      Node(
        name: $0.name, path: $0.path, isDirectory: $0.kind == .directory, ignored: $0.ignored)
    }
    let kept = Set(nodes.map(\.path))
    for old in children[listing.path] ?? [] where !kept.contains(old.path) {
      forget(old.path)
    }
    children[listing.path] = nodes
    if listing.truncated {
      truncated.insert(listing.path)
    } else {
      truncated.remove(listing.path)
    }
  }

  /// Expands a folder and reports whether it still has to be listed.
  @discardableResult
  public mutating func expand(_ directory: String) -> Bool {
    expanded.insert(directory)
    return children[directory] == nil
  }

  /// Closes a folder. Nested folders stay expanded for when it reopens, but
  /// their listings go: nothing watches a hidden folder, so they would be stale.
  public mutating func collapse(_ directory: String) {
    guard directory != Self.root else { return }
    expanded.remove(directory)
    let prefix = directory + "/"
    children = children.filter { $0.key != directory && !$0.key.hasPrefix(prefix) }
  }

  @discardableResult
  public mutating func toggle(_ directory: String) -> Bool {
    if expanded.contains(directory), directory != Self.root {
      collapse(directory)
      return false
    }
    return expand(directory)
  }

  /// Drops a folder's listing so the next load reads it again.
  public mutating func invalidate(_ directory: String) {
    children[directory] = nil
  }

  /// Switching ignored files forgets every listing; expanded folders reload.
  public mutating func setShowIgnored(_ show: Bool) {
    guard show != showIgnored else { return }
    showIgnored = show
    children = [:]
    truncated = []
  }

  /// Expands every folder above `path` and selects it, as Zed's
  /// `auto_reveal_entries` does. Returns the folders that need a listing.
  @discardableResult
  public mutating func reveal(_ path: String) -> [String] {
    var directory = Self.parent(of: path)
    while directory != Self.root {
      expanded.insert(directory)
      directory = Self.parent(of: directory)
    }
    selection = path
    return directoriesToLoad
  }

  /// Moves the selection or opens and closes folders. Returns a folder that
  /// must be listed before the move can show its children.
  @discardableResult
  public mutating func handle(_ key: Key) -> String? {
    let rows = rows
    guard !rows.isEmpty else { return nil }
    let index = selection.flatMap { path in rows.firstIndex { $0.node.path == path } }
    switch key {
    case .first: selection = rows.first?.node.path
    case .last: selection = rows.last?.node.path
    case .up:
      selection = rows[max(0, (index ?? rows.count) - 1)].node.path
    case .down:
      selection = rows[index.map { min(rows.count - 1, $0 + 1) } ?? 0].node.path
    case .right:
      guard let index, rows[index].node.isDirectory else { return nil }
      let path = rows[index].node.path
      if !rows[index].expanded { return expand(path) ? path : nil }
      if index + 1 < rows.count, rows[index + 1].depth > rows[index].depth {
        selection = rows[index + 1].node.path
      }
    case .left:
      guard let index else { return nil }
      if rows[index].expanded {
        collapse(rows[index].node.path)
      } else {
        let parent = Self.parent(of: rows[index].node.path)
        if parent != Self.root { selection = parent }
      }
    }
    return nil
  }

  public static func parent(of path: String) -> String {
    guard let slash = path.lastIndex(of: "/") else { return root }
    return String(path[..<slash])
  }

  /// Every folder above `path`, nearest first, ending with the root.
  public static func ancestors(of path: String) -> [String] {
    var folders: [String] = []
    var current = path
    while current != root {
      current = parent(of: current)
      folders.append(current)
    }
    return folders
  }

  public static func join(_ directory: String, _ name: String) -> String {
    directory.isEmpty ? name : "\(directory)/\(name)"
  }

  /// The folder new entries go into: the selected folder, or the folder of the
  /// selected file, or the root.
  public var creationDirectory: String {
    guard let selection else { return Self.root }
    if node(at: selection)?.isDirectory == true { return selection }
    return Self.parent(of: selection)
  }

  /// A folder is reachable when it is the root or every folder above it is expanded.
  private func isReachable(_ directory: String) -> Bool {
    var current = directory
    while current != Self.root {
      guard expanded.contains(current) else { return false }
      current = Self.parent(of: current)
    }
    return true
  }

  private mutating func forget(_ path: String) {
    let prefix = path + "/"
    expanded = expanded.filter { $0 != path && !$0.hasPrefix(prefix) }
    children = children.filter { $0.key != path && !$0.key.hasPrefix(prefix) }
    if let selection, selection == path || selection.hasPrefix(prefix) { self.selection = nil }
  }
}
