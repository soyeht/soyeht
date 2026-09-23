import Testing

@testable import SoyehtClient

@Test func paneDragUsesOnlyItsPrivateType() {
  #expect(PaneDragTransferPolicy.typeIdentifier == "com.soyeht.pane-drag")
  #expect(PaneDragTransferPolicy.accepts([PaneDragTransferPolicy.typeIdentifier]))
  #expect(!PaneDragTransferPolicy.accepts(["public.utf8-plain-text"]))
  #expect(!PaneDragTransferPolicy.accepts(["public.file-url"]))
}

@Test func chooserDropIsASilentNoOpWithoutTargetFeedback() {
  let chooser = PaneLayoutDropPresentationPolicy.plan(for: .chooser)
  let pane = PaneLayoutDropPresentationPolicy.plan(for: .pane)

  #expect(chooser.acceptsDrop)
  #expect(!chooser.showsTarget)
  #expect(chooser.completesAsNoOp)
  #expect(pane.acceptsDrop)
  #expect(pane.showsTarget)
  #expect(!pane.completesAsNoOp)
}

@Test func dropHitTestingUsesFourTwentyFivePercentEdgesAndACenter() throws {
  let area = PaneLayoutRect(size: PaneLayoutSize(width: 600, height: 500))
  let pane = PaneLayoutRect(
    origin: PaneLayoutPoint(x: 100, y: 100),
    size: PaneLayoutSize(width: 400, height: 200))
  let samples: [(PaneLayoutPoint, PaneLayoutDropZone)] = [
    (PaneLayoutPoint(x: 110, y: 200), .left),
    (PaneLayoutPoint(x: 490, y: 200), .right),
    (PaneLayoutPoint(x: 300, y: 110), .top),
    (PaneLayoutPoint(x: 300, y: 290), .bottom),
    (PaneLayoutPoint(x: 300, y: 200), .center),
    (PaneLayoutPoint(x: 110, y: 101), .top),
  ]

  for (point, expected) in samples {
    let target = try #require(
      PaneLayoutDropGeometry.target(
        at: point, sourceWorkspaceId: "workspace", targetWorkspaceId: "workspace",
        targetPaneId: "pane", paneArea: area, targetPaneRect: pane))
    #expect(target.zone == expected)
  }
}

@Test func dropOutsidePaneAreaOrAcrossWorkspaceIsAnExplicitNoOp() {
  let pane = PaneLayoutRect(
    origin: PaneLayoutPoint(x: 40, y: 40),
    size: PaneLayoutSize(width: 400, height: 300))
  let contentArea = PaneLayoutRect(
    origin: PaneLayoutPoint(x: 60, y: 100),
    size: PaneLayoutSize(width: 500, height: 400))

  #expect(
    PaneLayoutDropGeometry.target(
      at: PaneLayoutPoint(x: 200, y: 80), sourceWorkspaceId: "one",
      targetWorkspaceId: "one", targetPaneId: "pane", paneArea: contentArea,
      targetPaneRect: pane) == nil)
  #expect(
    PaneLayoutDropGeometry.target(
      at: PaneLayoutPoint(x: 50, y: 200), sourceWorkspaceId: "one",
      targetWorkspaceId: "one", targetPaneId: "pane", paneArea: contentArea,
      targetPaneRect: pane) == nil)
  #expect(
    PaneLayoutDropGeometry.target(
      at: PaneLayoutPoint(x: 200, y: 200), sourceWorkspaceId: "one",
      targetWorkspaceId: "two", targetPaneId: "pane", paneArea: contentArea,
      targetPaneRect: pane) == nil)
  #expect(
    PaneLayoutDropGeometry.target(
      at: PaneLayoutPoint(x: 550, y: 200), sourceWorkspaceId: "one",
      targetWorkspaceId: "one", targetPaneId: "pane", paneArea: contentArea,
      targetPaneRect: pane) == nil)
}

@Test func dropZoneRectsDeriveFromTheNamedFraction() {
  let pane = PaneLayoutRect(
    origin: PaneLayoutPoint(x: 100, y: 200),
    size: PaneLayoutSize(width: 400, height: 240))

  #expect(PaneLayoutDropGeometry.edgeFraction == 0.25)
  #expect(
    PaneLayoutDropGeometry.rect(for: .left, in: pane)
      == PaneLayoutRect(
        origin: PaneLayoutPoint(x: 100, y: 200),
        size: PaneLayoutSize(width: 100, height: 240)))
  #expect(
    PaneLayoutDropGeometry.rect(for: .bottom, in: pane)
      == PaneLayoutRect(
        origin: PaneLayoutPoint(x: 100, y: 380),
        size: PaneLayoutSize(width: 400, height: 60)))
  #expect(
    PaneLayoutDropGeometry.rect(for: .center, in: pane)
      == PaneLayoutRect(
        origin: PaneLayoutPoint(x: 200, y: 260),
        size: PaneLayoutSize(width: 200, height: 120)))
}

@Test func dividerHitAreasStayInsideGapsAndOutsidePaneFootprints() throws {
  let sizing = PaneLayoutSizingPolicy(
    floors: [
      .terminal: PaneLayoutSize(width: 300, height: 180),
      .editor: PaneLayoutSize(width: 600, height: 480),
    ],
    gaps: PaneLayoutGaps(horizontal: 20, vertical: 12))
  let tree = geometrySplit(
    .horizontal,
    geometryLeaf("editor", .editor),
    geometryLeaf("terminal", .terminal))
  let result = try PaneLayoutGeometry.layout(
    tree: tree, viewport: PaneLayoutSize(width: 1_100, height: 600), sizing: sizing)
  let divider = try #require(result.dividers.first)
  let editor = try #require(result.paneRects["editor"])
  let terminal = try #require(result.paneRects["terminal"])

  #expect(result.dividers.count == 1)
  #expect(divider.splitPath == PaneLayoutNodePath())
  #expect(divider.afterChildIndex == 0)
  #expect(divider.axis == .horizontal)
  #expect(divider.frame.size.width == sizing.gaps.horizontal)
  #expect(divider.frame.minX == editor.maxX)
  #expect(divider.frame.maxX == terminal.minX)
  #expect(!divider.frame.interactionIntersects(editor))
  #expect(!divider.frame.interactionIntersects(terminal))
}

@Test func nestedSplitsExposeAStableDividerPathForEachGap() throws {
  let sizing = PaneLayoutSizingPolicy(
    floors: [
      .terminal: PaneLayoutSize(width: 300, height: 180),
      .agent: PaneLayoutSize(width: 280, height: 220),
      .editor: PaneLayoutSize(width: 600, height: 480),
    ],
    gaps: PaneLayoutGaps(horizontal: 20, vertical: 12))
  let tree = geometrySplit(
    .horizontal,
    geometryLeaf("editor", .editor),
    geometrySplit(
      .vertical, geometryLeaf("terminal", .terminal), geometryLeaf("agent", .agent)))
  let result = try PaneLayoutGeometry.layout(
    tree: tree, viewport: PaneLayoutSize(width: 1_200, height: 700), sizing: sizing)

  #expect(result.dividers.count == 2)
  #expect(
    result.dividers.map(\.splitPath) == [
      PaneLayoutNodePath(), PaneLayoutNodePath(components: [1]),
    ])
  #expect(result.dividers.map(\.axis) == [.horizontal, .vertical])
}

private func geometryLeaf(_ paneId: String, _ kind: PaneLayoutKind) -> PaneLayoutTree {
  .leaf(PaneLayoutLeaf(paneId: paneId, kind: kind))
}

private func geometrySplit(
  _ axis: PaneLayoutAxis, _ children: PaneLayoutTree...
) -> PaneLayoutTree {
  .split(
    PaneLayoutSplit(
      axis: axis, children: children.map { PaneLayoutBranch(tree: $0) }))
}

extension PaneLayoutRect {
  fileprivate func interactionIntersects(_ other: PaneLayoutRect) -> Bool {
    minX < other.maxX && maxX > other.minX && minY < other.maxY && maxY > other.minY
  }
}
