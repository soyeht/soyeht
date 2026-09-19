public enum PaneLayoutProvisionalPlacement: Equatable, Sendable {
  case split(anchorPaneId: String, axis: PaneLayoutAxis)
  case arranged(rule: PaneLayoutInsertionRule)
}

/// A UI-only leaf inserted into a frozen base tree while a new pane is being configured.
///
/// Opening and cancelling this value never mutate persisted layout state. Completing it repeats
/// the same pure operation with the real pane ID, so preview and committed geometry cannot drift.
public struct PaneLayoutProvisionalInsertion: Equatable, Sendable {
  public var baseTree: PaneLayoutTree?
  public var placeholder: PaneLayoutLeaf
  public var placement: PaneLayoutProvisionalPlacement

  public init(
    baseTree: PaneLayoutTree?, placeholder: PaneLayoutLeaf,
    placement: PaneLayoutProvisionalPlacement
  ) {
    self.baseTree = baseTree
    self.placeholder = placeholder
    self.placement = placement
  }

  public func previewTree(sizing: PaneLayoutSizingPolicy) throws -> PaneLayoutTree {
    try inserting(placeholder, sizing: sizing)
  }

  public func completedTree(
    with leaf: PaneLayoutLeaf, sizing: PaneLayoutSizingPolicy
  ) throws -> PaneLayoutTree {
    try inserting(leaf, sizing: sizing)
  }

  public func selecting(kind: PaneLayoutKind) -> Self {
    var copy = self
    copy.placeholder.kind = kind
    return copy
  }

  private func inserting(
    _ leaf: PaneLayoutLeaf, sizing: PaneLayoutSizingPolicy
  ) throws -> PaneLayoutTree {
    switch placement {
    case .split(let anchorPaneId, let axis):
      guard let baseTree else { throw PaneLayoutMutationError.missingPane(anchorPaneId) }
      return try PaneLayoutTreeMutation.splitting(
        baseTree, anchorPaneId: anchorPaneId, newLeaf: leaf, axis: axis)
    case .arranged(let rule):
      return try PaneLayoutArrangement.inserting(
        leaf, into: baseTree, rule: rule, sizing: sizing)
    }
  }
}
