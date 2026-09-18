public enum PaneLayoutMutationError: Error, Equatable, Sendable {
  case duplicatePane(String)
  case missingPane(String)
  case invalidSplitPath(PaneLayoutNodePath)
  case invalidDivider(Int)
  case invalidDelta
  case invalidWeights
}

public enum PaneLayoutDropZone: String, CaseIterable, Equatable, Sendable {
  case left
  case right
  case top
  case bottom
  case center

  var axis: PaneLayoutAxis? {
    switch self {
    case .left, .right: .horizontal
    case .top, .bottom: .vertical
    case .center: nil
    }
  }

  var insertsBeforeTarget: Bool {
    self == .left || self == .top
  }
}

/// Pure changes to a pane tree. UI code supplies intentions and never edits branches directly.
public enum PaneLayoutTreeMutation {
  /// Adds a newly created pane immediately to the right of or below the anchor, taking half of
  /// its space above the panes' floors; each pane keeps its own floor. Normalization flattens a
  /// parent with the same axis.
  public static func splitting(
    _ tree: PaneLayoutTree, anchorPaneId: String, newLeaf: PaneLayoutLeaf,
    axis: PaneLayoutAxis
  ) throws -> PaneLayoutTree {
    guard tree.leaf(withId: anchorPaneId) != nil else {
      throw PaneLayoutMutationError.missingPane(anchorPaneId)
    }
    guard tree.leaf(withId: newLeaf.paneId) == nil else {
      throw PaneLayoutMutationError.duplicatePane(newLeaf.paneId)
    }
    let result = tree.replacingLeaf(withId: anchorPaneId) { anchor in
      PaneLayoutTree.split(
        PaneLayoutSplit(
          axis: axis,
          children: [
            PaneLayoutBranch(tree: anchor),
            PaneLayoutBranch(tree: .leaf(newLeaf)),
          ]))
    }
    guard result.replaced else { throw PaneLayoutMutationError.missingPane(anchorPaneId) }
    return result.tree.normalized()
  }

  /// Moves a pane to an edge of another pane, or swaps the two leaves for a center drop.
  public static func moving(
    _ tree: PaneLayoutTree, paneId: String, relativeTo targetPaneId: String,
    zone: PaneLayoutDropZone
  ) throws -> PaneLayoutTree {
    guard paneId != targetPaneId else { return tree }
    guard let source = tree.leaf(withId: paneId) else {
      throw PaneLayoutMutationError.missingPane(paneId)
    }
    guard tree.leaf(withId: targetPaneId) != nil else {
      throw PaneLayoutMutationError.missingPane(targetPaneId)
    }
    if zone == .center {
      return try swapping(tree, paneId, targetPaneId)
    }
    guard let axis = zone.axis,
      let pruned = tree.removingPane(withId: paneId)
    else {
      throw PaneLayoutMutationError.missingPane(paneId)
    }
    let inserted = pruned.replacingLeaf(withId: targetPaneId) { target in
      let children: [PaneLayoutTree] =
        zone.insertsBeforeTarget
        ? [.leaf(source), target]
        : [target, .leaf(source)]
      return .split(
        PaneLayoutSplit(
          axis: axis,
          children: children.map { PaneLayoutBranch(tree: $0) }))
    }
    guard inserted.replaced else {
      throw PaneLayoutMutationError.missingPane(targetPaneId)
    }
    return inserted.tree.normalized()
  }

  /// Exchanges two pane leaves without changing their surrounding branches or weights.
  public static func swapping(
    _ tree: PaneLayoutTree, _ firstPaneId: String, _ secondPaneId: String
  ) throws -> PaneLayoutTree {
    guard firstPaneId != secondPaneId else { return tree }
    guard let first = tree.leaf(withId: firstPaneId) else {
      throw PaneLayoutMutationError.missingPane(firstPaneId)
    }
    guard let second = tree.leaf(withId: secondPaneId) else {
      throw PaneLayoutMutationError.missingPane(secondPaneId)
    }
    return tree.mappingLeaves { leaf in
      switch leaf.paneId {
      case firstPaneId: second
      case secondPaneId: first
      default: leaf
      }
    }
  }

  /// Moves one divider in points. Positive deltas move right/down. Only the adjacent weights
  /// change, their sum stays fixed, and measured subtree floors clamp the result.
  public static func resizing(
    _ tree: PaneLayoutTree, splitAt path: PaneLayoutNodePath, dividerIndex: Int,
    delta: Double, viewport: PaneLayoutSize, sizing: PaneLayoutSizingPolicy
  ) throws -> PaneLayoutTree {
    guard delta.isFinite else { throw PaneLayoutMutationError.invalidDelta }
    let split = try tree.split(at: path)
    guard split.children.indices.contains(dividerIndex),
      split.children.indices.contains(dividerIndex + 1)
    else {
      throw PaneLayoutMutationError.invalidDivider(dividerIndex)
    }

    let geometry = try PaneLayoutGeometry.layout(tree: tree, viewport: viewport, sizing: sizing)
    let leftPath = path.appending(dividerIndex)
    let rightPath = path.appending(dividerIndex + 1)
    guard let splitGeometry = geometry.nodes[path],
      let leftGeometry = geometry.nodes[leftPath],
      let rightGeometry = geometry.nodes[rightPath]
    else {
      throw PaneLayoutMutationError.invalidSplitPath(path)
    }

    let leftMinimum = try PaneLayoutMinimumSize.require(
      for: split.children[dividerIndex].tree, floors: sizing.floors, gaps: sizing.gaps)
    let rightMinimum = try PaneLayoutMinimumSize.require(
      for: split.children[dividerIndex + 1].tree, floors: sizing.floors, gaps: sizing.gaps)
    let leftExtent = extent(of: leftGeometry.frame.size, axis: split.axis)
    let rightExtent = extent(of: rightGeometry.frame.size, axis: split.axis)
    let minimumDelta = extent(of: leftMinimum, axis: split.axis) - leftExtent
    let maximumDelta = rightExtent - extent(of: rightMinimum, axis: split.axis)
    let clampedDelta = min(max(delta, minimumDelta), maximumDelta)

    let minimums = try split.children.map {
      try PaneLayoutMinimumSize.require(for: $0.tree, floors: sizing.floors, gaps: sizing.gaps)
    }
    let gap = split.axis == .horizontal ? sizing.gaps.horizontal : sizing.gaps.vertical
    let minimumExtent =
      minimums.reduce(0) {
        $0 + extent(of: $1, axis: split.axis)
      } + gap * Double(max(0, split.children.count - 1))
    let contentExtent = extent(of: splitGeometry.contentSize, axis: split.axis)
    let distributable = max(0, contentExtent - minimumExtent)
    let weightTotal = split.children.reduce(0) { $0 + $1.weight }
    guard distributable > 0 else { return tree }
    guard weightTotal.isFinite, weightTotal > 0 else {
      throw PaneLayoutMutationError.invalidWeights
    }

    let requestedWeightDelta = clampedDelta * weightTotal / distributable
    let minimumWeight = max(weightTotal * 1e-9, Double.leastNormalMagnitude)
    let leftWeight = split.children[dividerIndex].weight
    let rightWeight = split.children[dividerIndex + 1].weight
    let weightDelta = min(
      max(requestedWeightDelta, minimumWeight - leftWeight),
      rightWeight - minimumWeight)
    guard weightDelta.isFinite, weightDelta != 0 else { return tree }

    return try tree.updatingSplit(at: path) { current in
      var children = current.children
      children[dividerIndex].weight += weightDelta
      children[dividerIndex + 1].weight -= weightDelta
      return PaneLayoutSplit(axis: current.axis, children: children)
    }
  }

  /// Makes direct children equally large in the current split when their measured floors allow it.
  /// A floor larger than the equal share wins and the remaining children share the rest.
  public static func equalizing(
    _ tree: PaneLayoutTree, splitAt path: PaneLayoutNodePath, viewport: PaneLayoutSize,
    sizing: PaneLayoutSizingPolicy
  ) throws -> PaneLayoutTree {
    let split = try tree.split(at: path)
    let geometry = try PaneLayoutGeometry.layout(tree: tree, viewport: viewport, sizing: sizing)
    guard let splitGeometry = geometry.nodes[path] else {
      throw PaneLayoutMutationError.invalidSplitPath(path)
    }
    let minimums = try split.children.map {
      try PaneLayoutMinimumSize.require(for: $0.tree, floors: sizing.floors, gaps: sizing.gaps)
    }
    let gap = split.axis == .horizontal ? sizing.gaps.horizontal : sizing.gaps.vertical
    let available = max(
      0,
      extent(of: splitGeometry.contentSize, axis: split.axis)
        - gap * Double(max(0, split.children.count - 1)))
    let desired = equalizedExtents(
      minimums.map { extent(of: $0, axis: split.axis) }, available: available)
    let flexible = zip(desired, minimums).map {
      max(0, $0.0 - extent(of: $0.1, axis: split.axis))
    }
    let distributable = flexible.reduce(0, +)
    let minimumWeight = max(distributable * 1e-9, Double.leastNormalMagnitude)
    let weights =
      distributable > 0
      ? flexible.map { max($0, minimumWeight) }
      : Array(repeating: PaneLayoutBranch.defaultWeight, count: split.children.count)

    return try tree.updatingSplit(at: path) { split in
      PaneLayoutSplit(
        axis: split.axis,
        children: zip(split.children, weights).map {
          PaneLayoutBranch(weight: $0.1, tree: $0.0.tree)
        })
    }
  }

  private static func equalizedExtents(_ minimums: [Double], available: Double) -> [Double] {
    var desired = minimums
    var unsettled = Set(minimums.indices)
    var remaining = available
    while !unsettled.isEmpty {
      let equalShare = remaining / Double(unsettled.count)
      let constrained = unsettled.filter { minimums[$0] > equalShare }
      guard !constrained.isEmpty else {
        for index in unsettled { desired[index] = equalShare }
        break
      }
      for index in constrained {
        desired[index] = minimums[index]
        remaining -= minimums[index]
        unsettled.remove(index)
      }
    }
    return desired
  }

  private static func extent(of size: PaneLayoutSize, axis: PaneLayoutAxis) -> Double {
    axis == .horizontal ? size.width : size.height
  }
}

extension PaneLayoutTree {
  fileprivate func leaf(withId paneId: String) -> PaneLayoutLeaf? {
    switch self {
    case .leaf(let leaf):
      leaf.paneId == paneId ? leaf : nil
    case .split(let split):
      split.children.lazy.compactMap { $0.tree.leaf(withId: paneId) }.first
    }
  }

  fileprivate func replacingLeaf(
    withId paneId: String, transform: (PaneLayoutTree) -> PaneLayoutTree
  ) -> (tree: PaneLayoutTree, replaced: Bool) {
    switch self {
    case .leaf(let leaf):
      guard leaf.paneId == paneId else { return (self, false) }
      return (transform(self), true)
    case .split(let split):
      var children = split.children
      for index in children.indices {
        let result = children[index].tree.replacingLeaf(withId: paneId, transform: transform)
        guard result.replaced else { continue }
        children[index].tree = result.tree
        return (.split(PaneLayoutSplit(axis: split.axis, children: children)), true)
      }
      return (self, false)
    }
  }

  fileprivate func mappingLeaves(
    _ transform: (PaneLayoutLeaf) -> PaneLayoutLeaf
  ) -> PaneLayoutTree {
    switch self {
    case .leaf(let leaf):
      .leaf(transform(leaf))
    case .split(let split):
      .split(
        PaneLayoutSplit(
          axis: split.axis,
          children: split.children.map {
            PaneLayoutBranch(weight: $0.weight, tree: $0.tree.mappingLeaves(transform))
          }))
    }
  }

  fileprivate func split(at path: PaneLayoutNodePath) throws -> PaneLayoutSplit {
    try split(remainingPath: path.components[...], originalPath: path)
  }

  private func split(
    remainingPath: ArraySlice<Int>, originalPath: PaneLayoutNodePath
  ) throws -> PaneLayoutSplit {
    guard let first = remainingPath.first else {
      guard case .split(let split) = self else {
        throw PaneLayoutMutationError.invalidSplitPath(originalPath)
      }
      return split
    }
    guard case .split(let split) = self, split.children.indices.contains(first) else {
      throw PaneLayoutMutationError.invalidSplitPath(originalPath)
    }
    return try split.children[first].tree.split(
      remainingPath: remainingPath.dropFirst(), originalPath: originalPath)
  }

  fileprivate func updatingSplit(
    at path: PaneLayoutNodePath, transform: (PaneLayoutSplit) throws -> PaneLayoutSplit
  ) throws -> PaneLayoutTree {
    try updatingSplit(
      remainingPath: path.components[...], originalPath: path, transform: transform)
  }

  private func updatingSplit(
    remainingPath: ArraySlice<Int>, originalPath: PaneLayoutNodePath,
    transform: (PaneLayoutSplit) throws -> PaneLayoutSplit
  ) throws -> PaneLayoutTree {
    guard let first = remainingPath.first else {
      guard case .split(let split) = self else {
        throw PaneLayoutMutationError.invalidSplitPath(originalPath)
      }
      return .split(try transform(split))
    }
    guard case .split(let split) = self, split.children.indices.contains(first) else {
      throw PaneLayoutMutationError.invalidSplitPath(originalPath)
    }
    var children = split.children
    children[first].tree = try children[first].tree.updatingSplit(
      remainingPath: remainingPath.dropFirst(), originalPath: originalPath, transform: transform)
    return .split(PaneLayoutSplit(axis: split.axis, children: children))
  }
}
