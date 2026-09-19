import Foundation
import SoyehtProtocol

public enum PaneLayoutPersistenceError: Error, Equatable, Sendable {
  case invalidTree
}

/// Workspace-scoped state. Viewport state such as scroll, rail visibility, and collapsed headers
/// deliberately stays outside this value because it belongs to one device.
public struct PaneLayoutState: Equatable, Sendable {
  public static let currentVersion: UInt32 = 2

  public var tree: PaneLayoutTree?
  public var mode: WorkspacePaneLayoutMode

  public init(tree: PaneLayoutTree? = nil, mode: WorkspacePaneLayoutMode = .automatic) {
    self.tree = tree
    self.mode = mode
  }

  public init(workspaceLayout: JsonValue) {
    guard let envelope = try? workspaceLayout.decoded(WorkspacePaneLayoutEnvelope.self),
      envelope.version == 1 || envelope.version == Self.currentVersion
    else {
      self.init()
      return
    }
    do {
      self.init(
        tree: try envelope.tree.map {
          try PaneLayoutTree(
            protocolNode: $0, includeCompanions: envelope.version == Self.currentVersion)
        },
        mode: envelope.mode)
    } catch {
      self.init()
    }
  }

  public init(workspaceLayout: JsonValue, livePanes: [Pane]) {
    self.init(workspaceLayout: workspaceLayout)
    var liveKinds: [String: PaneLayoutKind] = [:]
    for pane in livePanes {
      liveKinds[pane.paneId] = PaneLayoutKind(mode: pane.mode, writingKind: pane.writingKind)
    }
    guard var reconciled = tree else {
      if !liveKinds.isEmpty { self.init() }
      return
    }
    for paneId in reconciled.paneIdsInCanonicalOrder where liveKinds[paneId] == nil {
      guard let remaining = reconciled.removingPane(withId: paneId) else {
        tree = nil
        if !liveKinds.isEmpty { self.init() }
        return
      }
      reconciled = remaining
    }
    let leaves = reconciled.leavesInCanonicalOrder
    guard leaves.allSatisfy({ liveKinds[$0.paneId] == $0.kind }) else {
      self.init()
      return
    }
    tree = reconciled
  }

  /// Adds panes that appeared after a custom layout was stored without discarding that layout.
  /// The returned IDs are suitable for one structured diagnostic at the integration boundary.
  @discardableResult
  public mutating func reconcile(
    livePanes: [Pane], sizing: PaneLayoutSizingPolicy
  ) throws -> [String] {
    var present = Set(tree?.paneIdsInCanonicalOrder ?? [])
    var inserted: [String] = []
    for pane in livePanes where !present.contains(pane.paneId) {
      tree = try PaneLayoutArrangement.inserting(
        PaneLayoutLeaf(pane: pane), into: tree, rule: insertionRule, sizing: sizing)
      inserted.append(pane.paneId)
      present.insert(pane.paneId)
    }
    return inserted
  }

  public var insertionRule: PaneLayoutInsertionRule {
    mode == .custom ? .preservingUserArrangement : .initialByType
  }

  public mutating func recordUserArrangement(_ tree: PaneLayoutTree) {
    self.tree = tree
    mode = .custom
  }

  public func envelope() throws -> WorkspacePaneLayoutEnvelope {
    WorkspacePaneLayoutEnvelope(
      version: Self.currentVersion,
      mode: mode,
      tree: try tree.map { try WorkspacePaneLayoutNode(layoutTree: $0) })
  }

  public func encodedWorkspaceLayout() throws -> JsonValue {
    try .encoded(envelope())
  }
}

extension PaneLayoutTree {
  fileprivate init(
    protocolNode: WorkspacePaneLayoutNode, includeCompanions: Bool
  ) throws {
    switch protocolNode {
    case .v0(let leaf):
      guard !leaf.paneId.isEmpty else { throw PaneLayoutPersistenceError.invalidTree }
      self = .leaf(
        PaneLayoutLeaf(paneId: leaf.paneId, kind: PaneLayoutKind(protocolKind: leaf.kind)))
    case .v1(let split):
      guard split.children.count >= 2 else { throw PaneLayoutPersistenceError.invalidTree }
      let children = try split.children.map { branch in
        guard branch.weight.isFinite, branch.weight > 0 else {
          throw PaneLayoutPersistenceError.invalidTree
        }
        return PaneLayoutBranch(
          weight: branch.weight,
          tree: try PaneLayoutTree(
            protocolNode: branch.node, includeCompanions: includeCompanions))
      }
      let companion =
        includeCompanions
        ? split.companion.map {
          PaneLayoutCompanion(
            writingPaneId: $0.writingPaneId, agentPaneId: $0.agentPaneId)
        }
        : nil
      self = .split(
        PaneLayoutSplit(
          axis: PaneLayoutAxis(protocolAxis: split.axis), children: children,
          companion: companion))
    }
    let ids = paneIdsInCanonicalOrder
    guard Set(ids).count == ids.count else { throw PaneLayoutPersistenceError.invalidTree }
    var paired = Set<String>()
    guard companionsAreValid(paired: &paired) else {
      throw PaneLayoutPersistenceError.invalidTree
    }
  }

  private func companionsAreValid(paired: inout Set<String>) -> Bool {
    switch self {
    case .leaf:
      return true
    case .split(let split):
      guard split.children.allSatisfy({ $0.tree.companionsAreValid(paired: &paired) }) else {
        return false
      }
      guard let companion = split.companion else { return true }
      let leaves = leavesInCanonicalOrder
      let descendants = Set(leaves.map(\.paneId))
      let members = Set([companion.writingPaneId, companion.agentPaneId])
      guard members.count == 2, members.isSubset(of: descendants), paired.isDisjoint(with: members)
      else { return false }
      let directMembers = split.children.compactMap { child -> String? in
        guard case .leaf(let leaf) = child.tree else { return nil }
        return leaf.paneId
      }
      guard split.children.count == 2, directMembers.count == 2, Set(directMembers) == members
      else { return false }
      guard
        leaves.first(where: { $0.paneId == companion.writingPaneId }).map({
          [.text, .presentation, .screenplay].contains($0.kind)
        }) == true,
        leaves.first(where: { $0.paneId == companion.agentPaneId })?.kind == .agent
      else { return false }
      paired.formUnion(members)
      return true
    }
  }
}

extension WorkspacePaneLayoutNode {
  fileprivate init(layoutTree: PaneLayoutTree) throws {
    switch layoutTree {
    case .leaf(let leaf):
      self = .v0(
        WorkspacePaneLayoutLeaf(
          paneId: leaf.paneId, kind: WorkspacePaneLayoutKind(layoutKind: leaf.kind)))
    case .split(let split):
      guard split.children.count >= 2 else { throw PaneLayoutPersistenceError.invalidTree }
      self = .v1(
        WorkspacePaneLayoutSplit(
          axis: WorkspacePaneLayoutAxis(layoutAxis: split.axis),
          children: try split.children.map { branch in
            guard branch.weight.isFinite, branch.weight > 0 else {
              throw PaneLayoutPersistenceError.invalidTree
            }
            return WorkspacePaneLayoutBranch(
              weight: branch.weight, node: try WorkspacePaneLayoutNode(layoutTree: branch.tree))
          },
          companion: split.companion.map {
            WorkspacePaneLayoutCompanion(
              writingPaneId: $0.writingPaneId, agentPaneId: $0.agentPaneId)
          }))
    }
  }
}

extension PaneLayoutKind {
  fileprivate init(protocolKind: WorkspacePaneLayoutKind) {
    switch protocolKind {
    case .terminal: self = .terminal
    case .agent: self = .agent
    case .editor: self = .editor
    case .web: self = .web
    case .text: self = .text
    case .presentation: self = .presentation
    case .screenplay: self = .screenplay
    case .unknown_writing: self = .unknownWriting
    }
  }
}

extension WorkspacePaneLayoutKind {
  fileprivate init(layoutKind: PaneLayoutKind) {
    switch layoutKind {
    case .terminal: self = .terminal
    case .agent: self = .agent
    case .editor: self = .editor
    case .web: self = .web
    case .text: self = .text
    case .presentation: self = .presentation
    case .screenplay: self = .screenplay
    case .unknownWriting: self = .unknown_writing
    }
  }
}

extension PaneLayoutAxis {
  fileprivate init(protocolAxis: WorkspacePaneLayoutAxis) {
    self = protocolAxis == .horizontal ? .horizontal : .vertical
  }
}

extension WorkspacePaneLayoutAxis {
  fileprivate init(layoutAxis: PaneLayoutAxis) {
    self = layoutAxis == .horizontal ? .horizontal : .vertical
  }
}
