import Testing

@testable import SoyehtClient

private let provisionalSizing = PaneLayoutSizingPolicy(
  floors: [
    .terminal: PaneLayoutSize(width: 300, height: 180),
    .agent: PaneLayoutSize(width: 280, height: 220),
    .editor: PaneLayoutSize(width: 600, height: 480),
  ],
  gaps: PaneLayoutGaps(horizontal: 20, vertical: 12))

@Test func splitPreviewAndCompletionUseTheSameOperationAndKeepEveryExistingPane() throws {
  let base = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(weight: 3, tree: provisionalLeaf("editor", .editor)),
        PaneLayoutBranch(weight: 1, tree: provisionalLeaf("terminal", .terminal)),
      ]))
  let insertion = PaneLayoutProvisionalInsertion(
    baseTree: base,
    placeholder: PaneLayoutLeaf(paneId: "placeholder", kind: .terminal),
    placement: .split(anchorPaneId: "terminal", axis: .vertical))

  let preview = try insertion.previewTree(sizing: provisionalSizing)
  let completed = try insertion.completedTree(
    with: PaneLayoutLeaf(paneId: "created", kind: .terminal), sizing: provisionalSizing)

  #expect(preview.paneIdsInCanonicalOrder == ["editor", "terminal", "placeholder"])
  #expect(completed.paneIdsInCanonicalOrder == ["editor", "terminal", "created"])
  #expect(provisionalShape(preview, replacing: "placeholder") == provisionalShape(completed, replacing: "created"))
}

@Test func globalPreviewUsesTheCurrentArrangementRuleAndKeepsItsBaseFrozen() throws {
  let base = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(tree: provisionalLeaf("editor", .editor)),
        PaneLayoutBranch(tree: provisionalLeaf("terminal", .terminal)),
      ]))
  let insertion = PaneLayoutProvisionalInsertion(
    baseTree: base,
    placeholder: PaneLayoutLeaf(paneId: "placeholder", kind: .agent),
    placement: .arranged(rule: .preservingUserArrangement))

  let preview = try insertion.previewTree(sizing: provisionalSizing)
  let completed = try insertion.completedTree(
    with: PaneLayoutLeaf(paneId: "created", kind: .agent), sizing: provisionalSizing)

  #expect(preview.paneIdsInCanonicalOrder == ["editor", "terminal", "placeholder"])
  #expect(provisionalShape(preview, replacing: "placeholder") == provisionalShape(completed, replacing: "created"))
  #expect(insertion.baseTree == base)
}

@Test func emptyWorkspacePreviewIsOnePlaceholderLeaf() throws {
  let insertion = PaneLayoutProvisionalInsertion(
    baseTree: nil,
    placeholder: PaneLayoutLeaf(paneId: "placeholder", kind: .terminal),
    placement: .arranged(rule: .initialByType))

  #expect(try insertion.previewTree(sizing: provisionalSizing) == provisionalLeaf("placeholder", .terminal))
  #expect(
    try insertion.completedTree(
      with: PaneLayoutLeaf(paneId: "created", kind: .terminal), sizing: provisionalSizing)
      == provisionalLeaf("created", .terminal))
}

@Test func selectingChooserModeKeepsPlaceholderIdentityAndChangesItsMeasuredKind() throws {
  let insertion = PaneLayoutProvisionalInsertion(
    baseTree: provisionalLeaf("terminal", .terminal),
    placeholder: PaneLayoutLeaf(paneId: "placeholder", kind: .terminal),
    placement: .arranged(rule: .initialByType))

  let selected = insertion.selecting(kind: .agent)

  #expect(selected.placeholder == PaneLayoutLeaf(paneId: "placeholder", kind: .agent))
  #expect(insertion.placeholder.kind == .terminal)
}

@Test func paneSpecificFloorAppliesOnlyToThePlaceholderLeaf() throws {
  let tree = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(tree: provisionalLeaf("real", .agent)),
        PaneLayoutBranch(tree: provisionalLeaf("placeholder", .agent)),
      ]))
  let sizing = provisionalSizing.overridingFloor(
    forPaneId: "placeholder", with: PaneLayoutSize(width: 370, height: 432))
  let plan = try PaneLayoutGeometry.layout(
    tree: tree, viewport: PaneLayoutSize(width: 0, height: 0), sizing: sizing)

  #expect(plan.paneRects["real"]?.size == PaneLayoutSize(width: 280, height: 220))
  #expect(plan.paneRects["placeholder"]?.size == PaneLayoutSize(width: 370, height: 432))
}

private func provisionalLeaf(_ id: String, _ kind: PaneLayoutKind) -> PaneLayoutTree {
  .leaf(PaneLayoutLeaf(paneId: id, kind: kind))
}

private func provisionalShape(_ tree: PaneLayoutTree, replacing paneId: String) -> String {
  switch tree {
  case .leaf(let leaf):
    return leaf.paneId == paneId ? "leaf:new:\(leaf.kind.rawValue)" : "leaf:\(leaf.paneId):\(leaf.kind.rawValue)"
  case .split(let split):
    return "\(split.axis.rawValue)[\(split.children.map { "\($0.weight):\(provisionalShape($0.tree, replacing: paneId))" }.joined(separator: ","))]"
  }
}
