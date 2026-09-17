import Foundation
import SoyehtProtocol

public enum PaneLayoutRole: String, Codable, Sendable {
  case primary
  case auxiliary
}

public enum PaneLayoutKind: String, CaseIterable, Codable, Hashable, Sendable {
  case terminal
  case agent
  case editor
  case web
  case text
  case presentation
  case screenplay
  case unknownWriting

  public init(mode: PaneMode, writingKind: WritingKind? = nil) {
    switch mode {
    case .terminal:
      self = .terminal
    case .agent:
      self = .agent
    case .editor:
      self = .editor
    case .web:
      self = .web
    case .writing:
      switch writingKind {
      case .text: self = .text
      case .presentation: self = .presentation
      case .screenplay: self = .screenplay
      case nil: self = .unknownWriting
      }
    }
  }

  public var role: PaneLayoutRole {
    switch self {
    case .terminal, .agent:
      .auxiliary
    case .editor, .web, .text, .presentation, .screenplay, .unknownWriting:
      .primary
    }
  }
}

public enum PaneLayoutAxis: String, Codable, Sendable {
  case horizontal
  case vertical
}

public struct PaneLayoutLeaf: Codable, Equatable, Sendable {
  public var paneId: String
  public var kind: PaneLayoutKind

  public init(paneId: String, kind: PaneLayoutKind) {
    self.paneId = paneId
    self.kind = kind
  }

  public init(pane: Pane) {
    self.init(
      paneId: pane.paneId,
      kind: PaneLayoutKind(mode: pane.mode, writingKind: pane.writingKind))
  }
}

public struct PaneLayoutBranch: Codable, Equatable, Sendable {
  public static let defaultWeight = 1.0

  public var weight: Double
  public var tree: PaneLayoutTree

  public init(weight: Double = Self.defaultWeight, tree: PaneLayoutTree) {
    self.weight = weight.isFinite && weight > 0 ? weight : Self.defaultWeight
    self.tree = tree
  }
}

public struct PaneLayoutSplit: Codable, Equatable, Sendable {
  public var axis: PaneLayoutAxis
  public var children: [PaneLayoutBranch]

  public init(axis: PaneLayoutAxis, children: [PaneLayoutBranch]) {
    self.axis = axis
    self.children = children
  }
}

public indirect enum PaneLayoutTree: Codable, Equatable, Sendable {
  case leaf(PaneLayoutLeaf)
  case split(PaneLayoutSplit)

  public var leavesInCanonicalOrder: [PaneLayoutLeaf] {
    switch self {
    case .leaf(let leaf):
      [leaf]
    case .split(let split):
      split.children.flatMap { $0.tree.leavesInCanonicalOrder }
    }
  }

  public var paneIdsInCanonicalOrder: [String] {
    leavesInCanonicalOrder.map(\.paneId)
  }

  public func removingPane(withId paneId: String) -> PaneLayoutTree? {
    switch self {
    case .leaf(let leaf):
      return leaf.paneId == paneId ? nil : self
    case .split(let split):
      let remaining = split.children.compactMap { branch -> PaneLayoutBranch? in
        guard let tree = branch.tree.removingPane(withId: paneId) else { return nil }
        return PaneLayoutBranch(weight: branch.weight, tree: tree)
      }
      switch remaining.count {
      case 0:
        return nil
      case 1:
        return remaining[0].tree
      default:
        return .split(PaneLayoutSplit(axis: split.axis, children: remaining))
      }
    }
  }

  fileprivate func containsOnly(role: PaneLayoutRole) -> Bool {
    let leaves = leavesInCanonicalOrder
    return !leaves.isEmpty && leaves.allSatisfy { $0.kind.role == role }
  }
}

public enum PaneLayoutInitialArrangement {
  public static func inserting(
    _ leaf: PaneLayoutLeaf,
    into tree: PaneLayoutTree?
  ) -> PaneLayoutTree {
    guard let tree else { return .leaf(leaf) }
    guard !tree.paneIdsInCanonicalOrder.contains(leaf.paneId) else { return tree }

    switch leaf.kind.role {
    case .primary:
      return insertingPrimary(leaf, into: tree)
    case .auxiliary:
      return insertingAuxiliary(leaf, into: tree)
    }
  }

  private static func insertingPrimary(
    _ leaf: PaneLayoutLeaf,
    into tree: PaneLayoutTree
  ) -> PaneLayoutTree {
    if let regions = separatedRegions(in: tree) {
      var children = regions.split.children
      children[regions.primaryIndex].tree = appending(
        leaf, to: children[regions.primaryIndex].tree, along: .horizontal)
      return .split(PaneLayoutSplit(axis: .horizontal, children: children))
    }
    if tree.containsOnly(role: .auxiliary) {
      return split(.horizontal, [.leaf(leaf), tree])
    }
    if tree.containsOnly(role: .primary) {
      return appending(leaf, to: tree, along: .horizontal)
    }
    return split(.horizontal, [tree, .leaf(leaf)])
  }

  private static func insertingAuxiliary(
    _ leaf: PaneLayoutLeaf,
    into tree: PaneLayoutTree
  ) -> PaneLayoutTree {
    if let regions = separatedRegions(in: tree) {
      var children = regions.split.children
      children[regions.auxiliaryIndex].tree = appending(
        leaf, to: children[regions.auxiliaryIndex].tree, along: .vertical)
      return .split(PaneLayoutSplit(axis: .horizontal, children: children))
    }
    if tree.containsOnly(role: .primary) {
      return split(.horizontal, [tree, .leaf(leaf)])
    }
    if tree.containsOnly(role: .auxiliary) {
      return appending(leaf, to: tree, along: .vertical)
    }
    return split(.horizontal, [tree, .leaf(leaf)])
  }

  private static func appending(
    _ leaf: PaneLayoutLeaf,
    to tree: PaneLayoutTree,
    along axis: PaneLayoutAxis
  ) -> PaneLayoutTree {
    if case .split(let existing) = tree, existing.axis == axis {
      var children = existing.children
      children.append(PaneLayoutBranch(tree: .leaf(leaf)))
      return .split(PaneLayoutSplit(axis: axis, children: children))
    }
    return split(axis, [tree, .leaf(leaf)])
  }

  private static func split(
    _ axis: PaneLayoutAxis,
    _ trees: [PaneLayoutTree]
  ) -> PaneLayoutTree {
    .split(
      PaneLayoutSplit(
        axis: axis,
        children: trees.map { PaneLayoutBranch(tree: $0) }))
  }

  private static func separatedRegions(
    in tree: PaneLayoutTree
  ) -> (split: PaneLayoutSplit, primaryIndex: Int, auxiliaryIndex: Int)? {
    guard case .split(let split) = tree,
      split.axis == .horizontal,
      split.children.count == 2,
      split.children[0].tree.containsOnly(role: .primary),
      split.children[1].tree.containsOnly(role: .auxiliary)
    else { return nil }
    return (split, 0, 1)
  }
}

public struct PaneLayoutSize: Equatable, Sendable {
  public var width: Double
  public var height: Double

  public init(width: Double, height: Double) {
    self.width = width
    self.height = height
  }
}

public struct PaneLayoutGaps: Equatable, Sendable {
  public var horizontal: Double
  public var vertical: Double

  public init(horizontal: Double, vertical: Double) {
    self.horizontal = horizontal
    self.vertical = vertical
  }
}

public enum PaneLayoutMinimumSize {
  public static func derive(
    for tree: PaneLayoutTree,
    floors: [PaneLayoutKind: PaneLayoutSize],
    gaps: PaneLayoutGaps
  ) -> PaneLayoutSize? {
    switch tree {
    case .leaf(let leaf):
      return floors[leaf.kind]
    case .split(let split):
      let childSizes = split.children.compactMap {
        derive(for: $0.tree, floors: floors, gaps: gaps)
      }
      guard childSizes.count == split.children.count, !childSizes.isEmpty else {
        return nil
      }
      let gapCount = Double(childSizes.count - 1)
      switch split.axis {
      case .horizontal:
        return PaneLayoutSize(
          width: childSizes.reduce(0) { $0 + $1.width } + gaps.horizontal * gapCount,
          height: childSizes.map(\.height).max() ?? 0)
      case .vertical:
        return PaneLayoutSize(
          width: childSizes.map(\.width).max() ?? 0,
          height: childSizes.reduce(0) { $0 + $1.height } + gaps.vertical * gapCount)
      }
    }
  }
}
