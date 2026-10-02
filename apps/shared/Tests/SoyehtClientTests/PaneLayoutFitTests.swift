import Testing

@testable import SoyehtClient

private let fonts = acceptedFontMetrics
private let halo = PaneLayoutSpacing.paneHalo
private let floors = PaneLayoutFloorFormula.footprintFloors(fonts: fonts, halo: halo)
private let compactFloors = PaneLayoutFloorFormula.compactFootprintFloors(fonts: fonts, halo: halo)
private let sizing = PaneLayoutSizingPolicy(
  floors: floors, gaps: PaneLayoutSpacing.gaps,
  compactFloors: compactFloors)
private let window = PaneLayoutWindowPolicy(
  minimumExercisedViewport: PaneLayoutSize(width: 375, height: 375),
  largestPaneFootprintWidth: floors.values.map(\.width).max() ?? 0,
  standardHorizontalInset: PaneLayoutSpacing.outerMargin, topInset: 8, topBarHeight: 42,
  paneVerticalInset: PaneLayoutSpacing.outerMargin,
  railWidth: 56)

/// The automatic layout the Mac builds as panes open, editor first.
private func workspace(_ kinds: [PaneLayoutKind]) throws -> PaneLayoutTree {
  var tree: PaneLayoutTree?
  for (index, kind) in kinds.enumerated() {
    tree = try PaneLayoutArrangement.inserting(
      PaneLayoutLeaf(paneId: "\(kind)-\(index)", kind: kind), into: tree,
      rule: .initialByType, sizing: sizing)
  }
  return try #require(tree)
}

/// Every pane of the stored tree lies inside the viewport with at least its own minimum: its
/// compact floor when it has one (the editor without its project panel), else its floor.
private func expectFits(
  _ result: PaneLayoutFitResult, of tree: PaneLayoutTree, in viewport: PaneLayoutSize
) throws {
  #expect(Set(result.plan.paneRects.keys) == Set(tree.paneIdsInCanonicalOrder))
  for leaf in tree.leavesInCanonicalOrder {
    let rect = try #require(result.plan.paneRects[leaf.paneId])
    #expect(rect.origin.x >= 0 && rect.origin.y >= 0, "\(leaf.paneId)")
    #expect(rect.origin.x + rect.size.width <= viewport.width, "\(leaf.paneId)")
    #expect(rect.origin.y + rect.size.height <= viewport.height, "\(leaf.paneId)")
    let own = try #require(compactFloors[leaf.kind] ?? floors[leaf.kind])
    #expect(rect.size.width >= own.width && rect.size.height >= own.height, "\(leaf.paneId)")
  }
  for node in result.plan.nodes.values { #expect(node.overflowAxes.isEmpty) }
}

private func viewport1024By700(railOpen: Bool) -> PaneLayoutSize {
  window.paneViewport(window: PaneLayoutSize(width: 1024, height: 700), railOpen: railOpen)
}

@Test(arguments: [false, true])
func editorAndAgentFitA1024By700Window(railOpen: Bool) throws {
  let viewport = viewport1024By700(railOpen: railOpen)
  let tree = try workspace([.editor, .agent])
  // The tight grid (`PaneLayoutSpacing`) leaves 920 points with the rail open, enough for the
  // editor's full floor, project panel included, beside the agent: 545 + 18 + 340 = 903.
  #expect(try sizing.minimumSize(for: tree).width <= viewport1024By700(railOpen: true).width)

  let result = try PaneLayoutFit.layout(tree: tree, viewport: viewport, sizing: sizing)
  try expectFits(result, of: tree, in: viewport)
  #expect(result.keepsArrangement)
  #expect(result.plan.tabGroup == nil)
  let editor = try #require(result.plan.paneRects["editor-0"])
  #expect(
    PaneLayoutFloorFormula.editorShowsProjectPanel(
      surfaceWidth: editor.size.width - 2 * halo, fonts: fonts))
}

@Test func editorAndAgentFitANarrowWindowWithACompactEditor() throws {
  let viewport = window.paneViewport(
    window: PaneLayoutSize(width: 900, height: 700), railOpen: true)
  let tree = try workspace([.editor, .agent])
  // 796 points cannot hold the full editor beside the agent; this is the overflow the fit
  // removes by dropping the project panel (333 + 18 + 340 = 691).
  #expect(try sizing.minimumSize(for: tree).width > viewport.width)

  let result = try PaneLayoutFit.layout(tree: tree, viewport: viewport, sizing: sizing)
  try expectFits(result, of: tree, in: viewport)
  #expect(result.keepsArrangement)
  #expect(result.plan.tabGroup == nil)
  let editor = try #require(result.plan.paneRects["editor-0"])
  #expect(editor.size.width < floors[.editor]!.width)
  #expect(
    !PaneLayoutFloorFormula.editorShowsProjectPanel(
      surfaceWidth: editor.size.width - 2 * halo, fonts: fonts))
}

@Test(arguments: [false, true])
func editorAgentAndTerminalFitA1024By700Window(railOpen: Bool) throws {
  let viewport = viewport1024By700(railOpen: railOpen)
  let tree = try workspace([.editor, .agent, .terminal])

  let result = try PaneLayoutFit.layout(tree: tree, viewport: viewport, sizing: sizing)
  // Agent and terminal cannot stack in 602 points, and the three panes need 1049 points side by
  // side even with a compact editor, so the two share one tab group beside the editor.
  try expectFits(result, of: tree, in: viewport)
  let group = try #require(result.plan.tabGroup)
  #expect(group.paneIds == ["agent-1", "terminal-2"])
  #expect(!result.keepsArrangement)
  #expect(
    group.stripRect.origin
      == result.plan.paneRects["editor-0"].map {
        PaneLayoutPoint(x: $0.size.width + sizing.gaps.horizontal, y: 0)
      })
  #expect(group.stripRect.size.height == PaneLayoutContentMetrics.companionTabStripHeight)
  #expect(group.paneRect.origin.y == group.stripRect.size.height)
}

@Test func roomyWindowKeepsTheStoredArrangementAndTheProjectPanel() throws {
  let viewport = window.paneViewport(
    window: PaneLayoutSize(width: 1600, height: 1000), railOpen: true)
  let tree = try workspace([.editor, .agent, .terminal])

  let result = try PaneLayoutFit.layout(tree: tree, viewport: viewport, sizing: sizing)
  #expect(result.keepsArrangement)
  #expect(result.tree == tree)
  #expect(result.sizing == sizing)
  #expect(
    result.plan == (try PaneLayoutGeometry.layout(tree: tree, viewport: viewport, sizing: sizing)))
  let editor = try #require(result.plan.paneRects["editor-0"])
  #expect(
    PaneLayoutFloorFormula.editorShowsProjectPanel(
      surfaceWidth: editor.size.width - 2 * halo, fonts: fonts))
}

@Test func panesThatFitSideBySideAreRearrangedBeforeTheyAreTabbed() throws {
  // Stacked agent and terminal overflow 602 points, but a row of the two fits.
  let viewport = viewport1024By700(railOpen: true)
  let tree = try workspace([.agent, .terminal])

  let result = try PaneLayoutFit.layout(tree: tree, viewport: viewport, sizing: sizing)
  try expectFits(result, of: tree, in: viewport)
  #expect(result.plan.tabGroup == nil)
  guard case .split(let split) = result.tree else {
    Issue.record("expected a row")
    return
  }
  #expect(split.axis == .horizontal)
}

@Test func compactFloorsLeaveEveryOtherKindAlone() {
  #expect(Array(compactFloors.keys) == [.editor])
  #expect(compactFloors[.editor]!.width == floors[.editor]!.width - 212)
  #expect(compactFloors[.editor]!.height == floors[.editor]!.height)
}
