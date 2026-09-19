import SoyehtProtocol

public enum PaneLayoutRole: String, Sendable {
  case primary
  case auxiliary
}

/// The in-memory pane classification used by layout policy.
///
/// Web panes are primary surfaces, alongside editors and writing panes. Layout persistence must
/// wrap this model in a versioned envelope before adding `Codable` conformance.
public enum PaneLayoutKind: String, CaseIterable, Hashable, Sendable {
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

public enum PaneLayoutAxis: String, Hashable, Sendable {
  case horizontal
  case vertical
}

public struct PaneLayoutLeaf: Equatable, Sendable {
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

public struct PaneLayoutBranch: Equatable, Sendable {
  public static let defaultWeight = 1.0

  public var weight: Double
  public var tree: PaneLayoutTree

  public init(weight: Double = Self.defaultWeight, tree: PaneLayoutTree) {
    self.weight = weight.isFinite && weight > 0 ? weight : Self.defaultWeight
    self.tree = tree
  }
}

public struct PaneLayoutSplit: Equatable, Sendable {
  public var axis: PaneLayoutAxis
  public var children: [PaneLayoutBranch]

  public init(axis: PaneLayoutAxis, children: [PaneLayoutBranch]) {
    self.axis = axis
    self.children = children
  }
}

public indirect enum PaneLayoutTree: Equatable, Sendable {
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
        return PaneLayoutTree.split(
          PaneLayoutSplit(axis: split.axis, children: remaining)
        ).normalized()
      }
    }
  }

  fileprivate func replacingLastLeaf(
    with role: PaneLayoutRole,
    transform: (PaneLayoutTree) throws -> PaneLayoutTree
  ) rethrows -> (tree: PaneLayoutTree, replaced: Bool) {
    switch self {
    case .leaf(let leaf):
      guard leaf.kind.role == role else { return (self, false) }
      return (try transform(self), true)
    case .split(let split):
      var children = split.children
      for index in children.indices.reversed() {
        let result = try children[index].tree.replacingLastLeaf(
          with: role, transform: transform)
        guard result.replaced else { continue }
        children[index].tree = result.tree
        return (.split(PaneLayoutSplit(axis: split.axis, children: children)), true)
      }
      return (self, false)
    }
  }

  /// Flattens child splits that use their parent's axis without changing the parent's share.
  func normalized() -> PaneLayoutTree {
    switch self {
    case .leaf:
      return self
    case .split(let split):
      var flattened: [PaneLayoutBranch] = []
      for branch in split.children {
        let child = branch.tree.normalized()
        guard case .split(let childSplit) = child, childSplit.axis == split.axis else {
          flattened.append(PaneLayoutBranch(weight: branch.weight, tree: child))
          continue
        }
        let childWeight = childSplit.children.reduce(0) { $0 + $1.weight }
        for nested in childSplit.children {
          flattened.append(
            PaneLayoutBranch(
              weight: branch.weight * nested.weight / childWeight,
              tree: nested.tree))
        }
      }
      switch flattened.count {
      case 0:
        return self
      case 1:
        return flattened[0].tree
      default:
        return .split(PaneLayoutSplit(axis: split.axis, children: flattened))
      }
    }
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

public enum PaneLayoutSizingError: Error, Equatable, Sendable {
  case missingFloor(PaneLayoutKind)
}

/// One named proportion rule: each branch receives the minimum width or height its subtree needs.
public struct PaneLayoutSizingPolicy: Equatable, Sendable {
  public var floors: [PaneLayoutKind: PaneLayoutSize]
  public var gaps: PaneLayoutGaps
  /// Presentation-only floors for leaves whose content differs from their eventual pane kind.
  /// They are keyed by pane ID so provisional UI never needs a persistable protocol kind.
  public var paneFloors: [String: PaneLayoutSize]

  public init(
    floors: [PaneLayoutKind: PaneLayoutSize], gaps: PaneLayoutGaps,
    paneFloors: [String: PaneLayoutSize] = [:]
  ) {
    self.floors = floors
    self.gaps = gaps
    self.paneFloors = paneFloors
  }

  public func overridingFloor(forPaneId paneId: String, with floor: PaneLayoutSize) -> Self {
    var copy = self
    copy.paneFloors[paneId] = floor
    return copy
  }

  fileprivate func branches(
    for trees: [PaneLayoutTree], along axis: PaneLayoutAxis
  ) throws -> [PaneLayoutBranch] {
    try trees.map { tree in
      let minimum = try minimumSize(for: tree)
      return PaneLayoutBranch(
        weight: axis == .horizontal ? minimum.width : minimum.height,
        tree: tree)
    }
  }

  fileprivate func reweighting(_ tree: PaneLayoutTree) throws -> PaneLayoutTree {
    switch tree {
    case .leaf:
      return tree
    case .split(let split):
      let children = try split.children.map { try reweighting($0.tree) }
      return .split(
        PaneLayoutSplit(axis: split.axis, children: try branches(for: children, along: split.axis)))
    }
  }

  func floor(for leaf: PaneLayoutLeaf) throws -> PaneLayoutSize {
    if let floor = paneFloors[leaf.paneId] { return floor }
    guard let floor = floors[leaf.kind] else {
      throw PaneLayoutSizingError.missingFloor(leaf.kind)
    }
    return floor
  }

  func minimumSize(for tree: PaneLayoutTree) throws -> PaneLayoutSize {
    switch tree {
    case .leaf(let leaf):
      return try floor(for: leaf)
    case .split(let split):
      let childSizes = try split.children.map { try minimumSize(for: $0.tree) }
      let gapCount = Double(max(0, childSizes.count - 1))
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

public enum PaneLayoutInsertionRule: Equatable, Sendable {
  /// P1: primary panes are laid out left-to-right; auxiliaries stack in the right region.
  case initialByType
  /// Preserve the user's existing shares and split the last canonical pane of the same role.
  case preservingUserArrangement
}

public enum PaneLayoutArrangement {
  public static func inserting(
    _ leaf: PaneLayoutLeaf,
    into tree: PaneLayoutTree?,
    rule: PaneLayoutInsertionRule,
    sizing: PaneLayoutSizingPolicy
  ) throws -> PaneLayoutTree {
    guard let tree else { return .leaf(leaf) }
    guard !tree.paneIdsInCanonicalOrder.contains(leaf.paneId) else { return tree }

    let axis = splitAxis(for: leaf.kind.role)
    let replacement = try tree.replacingLastLeaf(with: leaf.kind.role) { existing in
      try split(axis, [existing, .leaf(leaf)], sizing: sizing)
    }
    let inserted: PaneLayoutTree
    if replacement.replaced {
      inserted = replacement.tree.normalized()
    } else {
      let ordered =
        leaf.kind.role == .primary ? [PaneLayoutTree.leaf(leaf), tree] : [tree, .leaf(leaf)]
      inserted = try split(.horizontal, ordered, sizing: sizing).normalized()
    }

    switch rule {
    case .initialByType:
      return try sizing.reweighting(inserted)
    case .preservingUserArrangement:
      return inserted
    }
  }

  private static func splitAxis(for role: PaneLayoutRole) -> PaneLayoutAxis {
    role == .primary ? .horizontal : .vertical
  }

  private static func split(
    _ axis: PaneLayoutAxis,
    _ trees: [PaneLayoutTree],
    sizing: PaneLayoutSizingPolicy
  ) throws -> PaneLayoutTree {
    .split(
      PaneLayoutSplit(
        axis: axis,
        children: try sizing.branches(for: trees, along: axis)))
  }
}

public enum PaneLayoutMinimumSize {
  public static func derive(
    for tree: PaneLayoutTree,
    floors: [PaneLayoutKind: PaneLayoutSize],
    gaps: PaneLayoutGaps
  ) -> PaneLayoutSize? {
    try? require(for: tree, floors: floors, gaps: gaps)
  }

  public static func require(
    for tree: PaneLayoutTree,
    floors: [PaneLayoutKind: PaneLayoutSize],
    gaps: PaneLayoutGaps
  ) throws -> PaneLayoutSize {
    switch tree {
    case .leaf(let leaf):
      guard let floor = floors[leaf.kind] else {
        throw PaneLayoutSizingError.missingFloor(leaf.kind)
      }
      return floor
    case .split(let split):
      let childSizes = try split.children.map {
        try require(for: $0.tree, floors: floors, gaps: gaps)
      }
      let gapCount = Double(max(0, childSizes.count - 1))
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
