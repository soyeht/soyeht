public enum PaneLayoutMutationError: Error, Equatable, Sendable {
  case duplicatePane(String)
  case missingPane(String)
  case invalidSplitPath(PaneLayoutNodePath)
  case invalidDivider(Int)
  case invalidDelta
  case invalidWeights
  case invalidCompanion
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

  /// Creates the explicit two-leaf writing/agent pair. Its annotated split is retained even when
  /// its axis matches the parent, so no later renderer has to infer companionship by position.
  public static func splittingWritingCompanion(
    _ tree: PaneLayoutTree, writingPaneId: String, agentLeaf: PaneLayoutLeaf,
    axis: PaneLayoutAxis
  ) throws -> PaneLayoutTree {
    guard agentLeaf.kind == .agent,
      let writing = tree.leaf(withId: writingPaneId),
      [.text, .screenplay, .presentation].contains(writing.kind),
      tree.companion(ofWritingPane: writingPaneId) == nil
    else { throw PaneLayoutMutationError.invalidCompanion }
    guard tree.leaf(withId: agentLeaf.paneId) == nil else {
      throw PaneLayoutMutationError.duplicatePane(agentLeaf.paneId)
    }
    let companion = PaneLayoutCompanion(
      writingPaneId: writingPaneId, agentPaneId: agentLeaf.paneId)
    let result = tree.replacingLeaf(withId: writingPaneId) { writingTree in
      .split(
        PaneLayoutSplit(
          axis: axis,
          children: [
            PaneLayoutBranch(tree: writingTree),
            PaneLayoutBranch(tree: .leaf(agentLeaf)),
          ],
          companion: companion))
    }
    guard result.replaced else { throw PaneLayoutMutationError.missingPane(writingPaneId) }
    return result.tree.normalized()
  }

  /// Moves a pane to an edge of another pane, or swaps the two leaves for a center drop. A writing
  /// companion that loses one of its panes to the swap is no longer a pair, as for any other drop.
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
      return try swapping(tree, paneId, targetPaneId).normalized()
    }
    if let reorderedPair = tree.reorderingCompanionMembers(
      paneId: paneId, relativeTo: targetPaneId, zone: zone)
    {
      return reorderedPair.normalized()
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

    let leftMinimum = try sizing.minimumSize(for: split.children[dividerIndex].tree)
    let rightMinimum = try sizing.minimumSize(for: split.children[dividerIndex + 1].tree)
    let leftExtent = extent(of: leftGeometry.frame.size, axis: split.axis)
    let rightExtent = extent(of: rightGeometry.frame.size, axis: split.axis)
    let minimumDelta = extent(of: leftMinimum, axis: split.axis) - leftExtent
    let maximumDelta = rightExtent - extent(of: rightMinimum, axis: split.axis)
    let clampedDelta = min(max(delta, minimumDelta), maximumDelta)

    let minimums = try split.children.map { try sizing.minimumSize(for: $0.tree) }
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
      return PaneLayoutSplit(
        axis: current.axis, children: children, companion: current.companion)
    }
  }

  /// Resizes an adaptive writing companion along its currently presented axis while retaining the
  /// persisted split axis. Only its two weights change, so side/below transitions keep the ratio.
  public static func resizingPresentedCompanion(
    _ tree: PaneLayoutTree, splitAt path: PaneLayoutNodePath,
    presentationAxis: PaneLayoutAxis, dividerIndex: Int, delta: Double,
    viewport: PaneLayoutSize, sizing: PaneLayoutSizingPolicy
  ) throws -> PaneLayoutTree {
    guard delta.isFinite else { throw PaneLayoutMutationError.invalidDelta }
    let split = try tree.split(at: path)
    guard split.companion != nil else { throw PaneLayoutMutationError.invalidCompanion }
    guard split.children.indices.contains(dividerIndex),
      split.children.indices.contains(dividerIndex + 1)
    else { throw PaneLayoutMutationError.invalidDivider(dividerIndex) }

    let presented = PaneLayoutTree.split(
      PaneLayoutSplit(axis: presentationAxis, children: split.children))
    let geometry = try PaneLayoutGeometry.layout(
      tree: presented, viewport: viewport, sizing: sizing)
    let root = PaneLayoutNodePath()
    guard let splitGeometry = geometry.nodes[root],
      let firstGeometry = geometry.nodes[root.appending(dividerIndex)],
      let secondGeometry = geometry.nodes[root.appending(dividerIndex + 1)]
    else { throw PaneLayoutMutationError.invalidSplitPath(path) }

    let minimums = try split.children.map { try sizing.minimumSize(for: $0.tree) }
    let firstExtent = extent(of: firstGeometry.frame.size, axis: presentationAxis)
    let secondExtent = extent(of: secondGeometry.frame.size, axis: presentationAxis)
    let minimumDelta = extent(of: minimums[dividerIndex], axis: presentationAxis) - firstExtent
    let maximumDelta =
      secondExtent - extent(of: minimums[dividerIndex + 1], axis: presentationAxis)
    let clampedDelta = min(max(delta, minimumDelta), maximumDelta)
    let gap = presentationAxis == .horizontal ? sizing.gaps.horizontal : sizing.gaps.vertical
    let minimumExtent =
      minimums.reduce(0) {
        $0 + extent(of: $1, axis: presentationAxis)
      } + gap * Double(max(0, split.children.count - 1))
    let contentExtent = extent(of: splitGeometry.contentSize, axis: presentationAxis)
    let distributable = max(0, contentExtent - minimumExtent)
    let weightTotal = split.children.reduce(0) { $0 + $1.weight }
    guard distributable > 0 else { return tree }
    guard weightTotal.isFinite, weightTotal > 0 else {
      throw PaneLayoutMutationError.invalidWeights
    }

    let requestedWeightDelta = clampedDelta * weightTotal / distributable
    let minimumWeight = max(weightTotal * 1e-9, Double.leastNormalMagnitude)
    let firstWeight = split.children[dividerIndex].weight
    let secondWeight = split.children[dividerIndex + 1].weight
    let weightDelta = min(
      max(requestedWeightDelta, minimumWeight - firstWeight),
      secondWeight - minimumWeight)
    guard weightDelta.isFinite, weightDelta != 0 else { return tree }

    return try tree.updatingSplit(at: path) { current in
      var children = current.children
      children[dividerIndex].weight += weightDelta
      children[dividerIndex + 1].weight -= weightDelta
      return PaneLayoutSplit(
        axis: current.axis, children: children, companion: current.companion)
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
    let minimums = try split.children.map { try sizing.minimumSize(for: $0.tree) }
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
        },
        companion: split.companion)
    }
  }

  public static func equalizingPresentedCompanion(
    _ tree: PaneLayoutTree, splitAt path: PaneLayoutNodePath,
    presentationAxis: PaneLayoutAxis, viewport: PaneLayoutSize,
    sizing: PaneLayoutSizingPolicy
  ) throws -> PaneLayoutTree {
    let split = try tree.split(at: path)
    guard split.companion != nil else { throw PaneLayoutMutationError.invalidCompanion }
    let minimums = try split.children.map { try sizing.minimumSize(for: $0.tree) }
    let gap = presentationAxis == .horizontal ? sizing.gaps.horizontal : sizing.gaps.vertical
    let available = max(
      0,
      extent(of: viewport, axis: presentationAxis)
        - gap * Double(max(0, split.children.count - 1)))
    let desired = equalizedExtents(
      minimums.map { extent(of: $0, axis: presentationAxis) }, available: available)
    let flexible = zip(desired, minimums).map {
      max(0, $0.0 - extent(of: $0.1, axis: presentationAxis))
    }
    let distributable = flexible.reduce(0, +)
    let minimumWeight = max(distributable * 1e-9, Double.leastNormalMagnitude)
    let weights =
      distributable > 0
      ? flexible.map { max($0, minimumWeight) }
      : Array(repeating: PaneLayoutBranch.defaultWeight, count: split.children.count)
    return try tree.updatingSplit(at: path) { current in
      PaneLayoutSplit(
        axis: current.axis,
        children: zip(current.children, weights).map {
          PaneLayoutBranch(weight: $0.1, tree: $0.0.tree)
        },
        companion: current.companion)
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
  fileprivate func reorderingCompanionMembers(
    paneId: String, relativeTo targetPaneId: String, zone: PaneLayoutDropZone
  ) -> PaneLayoutTree? {
    guard let axis = zone.axis else { return nil }
    switch self {
    case .leaf:
      return nil
    case .split(let split):
      if let companion = split.companion,
        Set([paneId, targetPaneId])
          == Set([companion.writingPaneId, companion.agentPaneId])
      {
        guard
          let source = split.children.first(where: {
            $0.tree.paneIdsInCanonicalOrder == [paneId]
          }),
          let target = split.children.first(where: {
            $0.tree.paneIdsInCanonicalOrder == [targetPaneId]
          })
        else { return nil }
        let children = zone.insertsBeforeTarget ? [source, target] : [target, source]
        return .split(
          PaneLayoutSplit(axis: axis, children: children, companion: companion))
      }
      for index in split.children.indices {
        guard
          let reordered = split.children[index].tree.reorderingCompanionMembers(
            paneId: paneId, relativeTo: targetPaneId, zone: zone)
        else { continue }
        var children = split.children
        children[index].tree = reordered
        return .split(
          PaneLayoutSplit(
            axis: split.axis, children: children, companion: split.companion))
      }
      return nil
    }
  }

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
        return (
          .split(
            PaneLayoutSplit(
              axis: split.axis, children: children, companion: split.companion)),
          true
        )
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
          },
          companion: split.companion))
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
    return .split(
      PaneLayoutSplit(axis: split.axis, children: children, companion: split.companion))
  }
}
