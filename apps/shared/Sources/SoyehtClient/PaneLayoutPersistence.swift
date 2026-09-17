import Foundation
import SoyehtProtocol

public enum PaneLayoutPersistenceError: Error, Equatable, Sendable {
  case invalidTree
}

/// Workspace-scoped state. Viewport state such as scroll, rail visibility, and collapsed headers
/// deliberately stays outside this value because it belongs to one device.
public struct PaneLayoutState: Equatable, Sendable {
  public static let currentVersion: UInt32 = 1

  public var tree: PaneLayoutTree?
  public var mode: WorkspacePaneLayoutMode

  public init(tree: PaneLayoutTree? = nil, mode: WorkspacePaneLayoutMode = .automatic) {
    self.tree = tree
    self.mode = mode
  }

  public init(workspaceLayout: JsonValue) {
    guard let envelope = try? workspaceLayout.decoded(WorkspacePaneLayoutEnvelope.self),
      envelope.version == Self.currentVersion
    else {
      self.init()
      return
    }
    do {
      self.init(
        tree: try envelope.tree.map(PaneLayoutTree.init(protocolNode:)), mode: envelope.mode)
    } catch {
      self.init()
    }
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
  fileprivate init(protocolNode: WorkspacePaneLayoutNode) throws {
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
          weight: branch.weight, tree: try PaneLayoutTree(protocolNode: branch.node))
      }
      self = .split(
        PaneLayoutSplit(axis: PaneLayoutAxis(protocolAxis: split.axis), children: children))
    }
    let ids = paneIdsInCanonicalOrder
    guard Set(ids).count == ids.count else { throw PaneLayoutPersistenceError.invalidTree }
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
