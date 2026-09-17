import Testing

@testable import SoyehtClient

private let sizing = PaneLayoutSizingPolicy(
  floors: [
    .terminal: PaneLayoutSize(width: 320, height: 180),
    .agent: PaneLayoutSize(width: 300, height: 220),
    .editor: PaneLayoutSize(width: 620, height: 480),
    .web: PaneLayoutSize(width: 560, height: 440),
    .text: PaneLayoutSize(width: 660, height: 500),
  ],
  gaps: PaneLayoutGaps(horizontal: 20, vertical: 12))

@Test func paneKindsMapProtocolModesToLayoutRoles() {
  #expect(PaneLayoutKind(mode: .editor).role == .primary)
  #expect(PaneLayoutKind(mode: .web).role == .primary)
  #expect(PaneLayoutKind(mode: .writing, writingKind: .text) == .text)
  #expect(PaneLayoutKind(mode: .writing, writingKind: .presentation) == .presentation)
  #expect(PaneLayoutKind(mode: .writing, writingKind: .screenplay) == .screenplay)
  #expect(PaneLayoutKind(mode: .writing).role == .primary)
  #expect(PaneLayoutKind(mode: .terminal).role == .auxiliary)
  #expect(PaneLayoutKind(mode: .agent).role == .auxiliary)
}

@Test func p1PlacesFloorWeightedPrimariesLeftAndStacksAuxiliariesRight() throws {
  var tree: PaneLayoutTree?
  for leaf in [
    PaneLayoutLeaf(paneId: "terminal", kind: .terminal),
    PaneLayoutLeaf(paneId: "agent", kind: .agent),
    PaneLayoutLeaf(paneId: "editor", kind: .editor),
    PaneLayoutLeaf(paneId: "text", kind: .text),
  ] {
    tree = try PaneLayoutArrangement.inserting(
      leaf, into: tree, rule: .initialByType, sizing: sizing)
  }

  let root = try #require(tree)
  let rootSplit = try #require(root.horizontalSplit)
  #expect(rootSplit.children.count == 3)
  #expect(
    rootSplit.children.map(\.tree.paneIdsInCanonicalOrder) == [
      ["editor"], ["text"], ["terminal", "agent"],
    ])
  #expect(rootSplit.children.map(\.weight) == [620, 660, 320])
  let auxiliaries = try #require(rootSplit.children[2].tree.verticalSplit)
  #expect(auxiliaries.children.map(\.weight) == [180, 220])
  #expect(root.paneIdsInCanonicalOrder == ["editor", "text", "terminal", "agent"])
}

@Test func sameAxisSplitsAreFlattenedInsteadOfNesting() throws {
  var tree: PaneLayoutTree?
  for id in ["editor-one", "editor-two", "editor-three"] {
    tree = try PaneLayoutArrangement.inserting(
      PaneLayoutLeaf(paneId: id, kind: .editor),
      into: tree,
      rule: .initialByType,
      sizing: sizing)
  }

  let split = try #require(tree?.horizontalSplit)
  #expect(split.children.count == 3)
  #expect(split.children.allSatisfy { $0.tree.horizontalSplit == nil })
  #expect(split.children.map(\.weight) == [620, 620, 620])
}

@Test func postDragInsertionUsesLastSameRoleLeafAndPreservesExistingShares() throws {
  let left = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .vertical,
      children: [
        PaneLayoutBranch(
          weight: 2, tree: .leaf(PaneLayoutLeaf(paneId: "editor", kind: .editor))),
        PaneLayoutBranch(
          weight: 1, tree: .leaf(PaneLayoutLeaf(paneId: "terminal", kind: .terminal))),
      ]))
  let custom = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(weight: 3, tree: left),
        PaneLayoutBranch(
          weight: 2, tree: .leaf(PaneLayoutLeaf(paneId: "web", kind: .web))),
      ]))

  let result = try PaneLayoutArrangement.inserting(
    PaneLayoutLeaf(paneId: "agent", kind: .agent),
    into: custom,
    rule: .preservingUserArrangement,
    sizing: sizing)

  let root = try #require(result.horizontalSplit)
  #expect(root.children.count == 2)
  #expect(root.children.map(\.weight) == [3, 2])
  #expect(result.paneIdsInCanonicalOrder == ["editor", "terminal", "agent", "web"])
  let leftAfter = try #require(root.children[0].tree.verticalSplit)
  #expect(leftAfter.children.count == 3)
  #expect(abs(leftAfter.children.reduce(0) { $0 + $1.weight } - 3) < 0.000_001)
}

@Test func insertionDoesNotDuplicateAnExistingPane() throws {
  let terminal = PaneLayoutLeaf(paneId: "terminal", kind: .terminal)
  let initial = try PaneLayoutArrangement.inserting(
    terminal, into: nil, rule: .initialByType, sizing: sizing)
  let repeated = try PaneLayoutArrangement.inserting(
    terminal, into: initial, rule: .preservingUserArrangement, sizing: sizing)

  #expect(repeated == initial)
}

@Test func closingPaneCollapsesAndNormalizesSplitsWithoutMovingLeaves() throws {
  var tree = try PaneLayoutArrangement.inserting(
    PaneLayoutLeaf(paneId: "editor", kind: .editor),
    into: nil,
    rule: .initialByType,
    sizing: sizing)
  tree = try PaneLayoutArrangement.inserting(
    PaneLayoutLeaf(paneId: "terminal", kind: .terminal),
    into: tree,
    rule: .initialByType,
    sizing: sizing)
  tree = try PaneLayoutArrangement.inserting(
    PaneLayoutLeaf(paneId: "agent", kind: .agent),
    into: tree,
    rule: .initialByType,
    sizing: sizing)

  let withoutAgent = try #require(tree.removingPane(withId: "agent"))
  #expect(withoutAgent.paneIdsInCanonicalOrder == ["editor", "terminal"])
  #expect(withoutAgent.horizontalSplit?.children[1].tree.paneIdsInCanonicalOrder == ["terminal"])

  let editorOnly = try #require(withoutAgent.removingPane(withId: "terminal"))
  #expect(editorOnly == .leaf(PaneLayoutLeaf(paneId: "editor", kind: .editor)))
  #expect(editorOnly.removingPane(withId: "editor") == nil)
}

@Test func canonicalOrderFollowsTreeGeometryAndNeverSelectionHistory() {
  let tree = PaneLayoutTree.split(
    axis: .horizontal,
    PaneLayoutTree.split(
      axis: .vertical,
      .leaf(PaneLayoutLeaf(paneId: "top-left", kind: .editor)),
      .leaf(PaneLayoutLeaf(paneId: "bottom-left", kind: .terminal))),
    PaneLayoutTree.split(
      axis: .vertical,
      .leaf(PaneLayoutLeaf(paneId: "top-right", kind: .agent)),
      .leaf(PaneLayoutLeaf(paneId: "bottom-right", kind: .web))))

  #expect(
    tree.paneIdsInCanonicalOrder
      == ["top-left", "bottom-left", "top-right", "bottom-right"])
}

@Test func derivedMinimumChangesWhenOneMeasuredFloorChanges() throws {
  var tree = try PaneLayoutArrangement.inserting(
    PaneLayoutLeaf(paneId: "terminal", kind: .terminal),
    into: nil,
    rule: .initialByType,
    sizing: sizing)
  tree = try PaneLayoutArrangement.inserting(
    PaneLayoutLeaf(paneId: "agent", kind: .agent),
    into: tree,
    rule: .initialByType,
    sizing: sizing)
  tree = try PaneLayoutArrangement.inserting(
    PaneLayoutLeaf(paneId: "editor", kind: .editor),
    into: tree,
    rule: .initialByType,
    sizing: sizing)

  let gaps = PaneLayoutGaps(horizontal: 20, vertical: 12)
  var floors: [PaneLayoutKind: PaneLayoutSize] = [
    .editor: PaneLayoutSize(width: 600, height: 500),
    .terminal: PaneLayoutSize(width: 320, height: 200),
    .agent: PaneLayoutSize(width: 300, height: 240),
  ]
  let initial = try #require(
    PaneLayoutMinimumSize.derive(for: tree, floors: floors, gaps: gaps))
  #expect(initial == PaneLayoutSize(width: 940, height: 500))

  floors[.editor] = PaneLayoutSize(width: 700, height: 500)
  let changed = try #require(
    PaneLayoutMinimumSize.derive(for: tree, floors: floors, gaps: gaps))
  #expect(changed == PaneLayoutSize(width: 1_040, height: 500))
  #expect(changed.width - initial.width == 100)
}

@Test func missingFloorRejectsInsertionInsteadOfFallingBackToEqualWeights() {
  let incomplete = PaneLayoutSizingPolicy(
    floors: [.editor: PaneLayoutSize(width: 620, height: 480)],
    gaps: PaneLayoutGaps(horizontal: 20, vertical: 12))
  let editor = PaneLayoutTree.leaf(PaneLayoutLeaf(paneId: "editor", kind: .editor))

  #expect(throws: PaneLayoutSizingError.missingFloor(.agent)) {
    try PaneLayoutArrangement.inserting(
      PaneLayoutLeaf(paneId: "agent", kind: .agent),
      into: editor,
      rule: .initialByType,
      sizing: incomplete)
  }
}

@Test func minimumIsUnavailableWhenAnyPaneFloorIsMissing() {
  let tree = PaneLayoutTree.split(
    axis: .horizontal,
    .leaf(PaneLayoutLeaf(paneId: "editor", kind: .editor)),
    .leaf(PaneLayoutLeaf(paneId: "agent", kind: .agent)))
  let floors: [PaneLayoutKind: PaneLayoutSize] = [
    .editor: PaneLayoutSize(width: 600, height: 500)
  ]

  #expect(
    PaneLayoutMinimumSize.derive(
      for: tree,
      floors: floors,
      gaps: PaneLayoutGaps(horizontal: 20, vertical: 12)) == nil)
}

extension PaneLayoutTree {
  fileprivate static func split(
    axis: PaneLayoutAxis,
    _ children: PaneLayoutTree...
  ) -> PaneLayoutTree {
    .split(
      PaneLayoutSplit(
        axis: axis,
        children: children.map { PaneLayoutBranch(tree: $0) }))
  }

  fileprivate var horizontalSplit: PaneLayoutSplit? {
    guard case .split(let split) = self, split.axis == .horizontal else { return nil }
    return split
  }

  fileprivate var verticalSplit: PaneLayoutSplit? {
    guard case .split(let split) = self, split.axis == .vertical else { return nil }
    return split
  }
}
