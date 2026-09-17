import Testing

@testable import SoyehtClient

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

@Test func p1PlacesPrimaryPanesLeftAndStacksAuxiliariesRight() throws {
  var tree: PaneLayoutTree?
  for leaf in [
    PaneLayoutLeaf(paneId: "terminal", kind: .terminal),
    PaneLayoutLeaf(paneId: "agent", kind: .agent),
    PaneLayoutLeaf(paneId: "editor", kind: .editor),
    PaneLayoutLeaf(paneId: "text", kind: .text),
  ] {
    tree = PaneLayoutInitialArrangement.inserting(leaf, into: tree)
  }

  let root = try #require(tree)
  let rootSplit = try #require(root.horizontalSplit)
  #expect(rootSplit.children.count == 2)
  #expect(rootSplit.children[0].tree.paneIdsInCanonicalOrder == ["editor", "text"])
  #expect(rootSplit.children[1].tree.verticalSplit != nil)
  #expect(rootSplit.children[1].tree.paneIdsInCanonicalOrder == ["terminal", "agent"])
  #expect(root.paneIdsInCanonicalOrder == ["editor", "text", "terminal", "agent"])
}

@Test func p1DoesNotDuplicateAnExistingPane() {
  let terminal = PaneLayoutLeaf(paneId: "terminal", kind: .terminal)
  let initial = PaneLayoutInitialArrangement.inserting(terminal, into: nil)
  let repeated = PaneLayoutInitialArrangement.inserting(terminal, into: initial)

  #expect(repeated == initial)
}

@Test func closingPaneCollapsesEmptySplitsWithoutMovingRemainingLeaves() throws {
  let editor = PaneLayoutLeaf(paneId: "editor", kind: .editor)
  let terminal = PaneLayoutLeaf(paneId: "terminal", kind: .terminal)
  let agent = PaneLayoutLeaf(paneId: "agent", kind: .agent)
  var tree = PaneLayoutInitialArrangement.inserting(editor, into: nil)
  tree = PaneLayoutInitialArrangement.inserting(terminal, into: tree)
  tree = PaneLayoutInitialArrangement.inserting(agent, into: tree)

  let withoutAgent = try #require(tree.removingPane(withId: "agent"))
  #expect(withoutAgent.paneIdsInCanonicalOrder == ["editor", "terminal"])
  #expect(withoutAgent.horizontalSplit?.children[1].tree == .leaf(terminal))

  let editorOnly = try #require(withoutAgent.removingPane(withId: "terminal"))
  #expect(editorOnly == .leaf(editor))
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
  var tree = PaneLayoutInitialArrangement.inserting(
    PaneLayoutLeaf(paneId: "terminal", kind: .terminal), into: nil)
  tree = PaneLayoutInitialArrangement.inserting(
    PaneLayoutLeaf(paneId: "agent", kind: .agent), into: tree)
  tree = PaneLayoutInitialArrangement.inserting(
    PaneLayoutLeaf(paneId: "editor", kind: .editor), into: tree)

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

@Test func minimumIsUnavailableWhenAnyPaneFloorIsMissing() {
  let tree = PaneLayoutInitialArrangement.inserting(
    PaneLayoutLeaf(paneId: "agent", kind: .agent),
    into: .leaf(PaneLayoutLeaf(paneId: "editor", kind: .editor)))
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
