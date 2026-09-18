import Testing

@testable import SoyehtClient

private let geometrySizing = PaneLayoutSizingPolicy(
  floors: [
    .terminal: PaneLayoutSize(width: 320, height: 180),
    .agent: PaneLayoutSize(width: 300, height: 220),
    .editor: PaneLayoutSize(width: 600, height: 480),
  ],
  gaps: PaneLayoutGaps(horizontal: 20, vertical: 12))

@Test func layoutRectsKeepFloorsAndScrollTheSplitAxisUnderPressure() throws {
  let tree = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(
          weight: 600, tree: .leaf(PaneLayoutLeaf(paneId: "editor", kind: .editor))),
        PaneLayoutBranch(
          weight: 320, tree: .leaf(PaneLayoutLeaf(paneId: "terminal", kind: .terminal))),
      ]))

  let result = try PaneLayoutGeometry.layout(
    tree: tree, viewport: PaneLayoutSize(width: 900, height: 500), sizing: geometrySizing)
  let root = try #require(result.nodes[PaneLayoutNodePath()])
  let editor = try #require(result.paneRects["editor"])
  let terminal = try #require(result.paneRects["terminal"])

  #expect(root.contentSize == PaneLayoutSize(width: 940, height: 500))
  #expect(root.overflowAxes == [.horizontal])
  #expect(editor.size == PaneLayoutSize(width: 600, height: 500))
  #expect(terminal.origin.x == 620)
  #expect(terminal.size == PaneLayoutSize(width: 320, height: 500))
}

@Test func surplusIsDistributedByPersistedWeightAfterEveryFloorIsPaid() throws {
  let tree = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(
          weight: 3, tree: .leaf(PaneLayoutLeaf(paneId: "editor", kind: .editor))),
        PaneLayoutBranch(
          weight: 1, tree: .leaf(PaneLayoutLeaf(paneId: "terminal", kind: .terminal))),
      ]))

  let rects = try PaneLayoutGeometry.layoutRects(
    tree: tree, viewport: PaneLayoutSize(width: 1_020, height: 500), sizing: geometrySizing)

  #expect(abs(try #require(rects["editor"]).size.width - 660) < 0.000_001)
  #expect(abs(try #require(rects["terminal"]).size.width - 340) < 0.000_001)
}

@Test func nestedAuxiliaryBranchOwnsItsVerticalOverflow() throws {
  let auxiliary = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .vertical,
      children: [
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "terminal", kind: .terminal))),
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "agent", kind: .agent))),
      ]))
  let tree = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "editor", kind: .editor))),
        PaneLayoutBranch(tree: auxiliary),
      ]))

  let result = try PaneLayoutGeometry.layout(
    tree: tree, viewport: PaneLayoutSize(width: 1_000, height: 300), sizing: geometrySizing)

  #expect(result.nodes[PaneLayoutNodePath()]?.overflowAxes.isEmpty == true)
  #expect(result.nodes[PaneLayoutNodePath(components: [1])]?.overflowAxes == [.vertical])
  #expect(result.nodes[PaneLayoutNodePath(components: [0])]?.overflowAxes == [.vertical])
  #expect(result.paneRects["editor"]?.size.height == 480)
}

@Test func paneIdentityPathsDoNotChangeWithViewportSize() throws {
  let tree = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "editor", kind: .editor))),
        PaneLayoutBranch(
          tree: .split(
            PaneLayoutSplit(
              axis: .vertical,
              children: [
                PaneLayoutBranch(
                  tree: .leaf(PaneLayoutLeaf(paneId: "terminal", kind: .terminal))),
                PaneLayoutBranch(
                  tree: .leaf(PaneLayoutLeaf(paneId: "agent", kind: .agent))),
              ]))),
      ]))

  let compact = try PaneLayoutGeometry.layout(
    tree: tree, viewport: PaneLayoutSize(width: 375, height: 375), sizing: geometrySizing)
  let wide = try PaneLayoutGeometry.layout(
    tree: tree, viewport: PaneLayoutSize(width: 1_440, height: 812), sizing: geometrySizing)

  #expect(compact.panePaths == wide.panePaths)
  #expect(compact.panePaths["editor"] == PaneLayoutNodePath(components: [0]))
  #expect(compact.panePaths["agent"] == PaneLayoutNodePath(components: [1, 1]))
}

@Test func onePointBelowEveryAxisProducesExplicitOverflow() throws {
  let tree = PaneLayoutTree.leaf(PaneLayoutLeaf(paneId: "terminal", kind: .terminal))
  let result = try PaneLayoutGeometry.layout(
    tree: tree, viewport: PaneLayoutSize(width: 319, height: 179), sizing: geometrySizing)
  let root = try #require(result.nodes[PaneLayoutNodePath()])

  #expect(root.contentSize == PaneLayoutSize(width: 320, height: 180))
  #expect(root.overflowAxes == [.horizontal, .vertical])
}
