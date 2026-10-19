import Testing

@testable import SoyehtClient

private let fonts = PaneLayoutFixture.fonts
private let halo = PaneLayoutFixture.halo
private let sizing = PaneLayoutFixture.sizing
private let window = PaneLayoutFixture.window(
  largestWidth: sizing.foldedFloors.values.map(\.width).max() ?? 0,
  smallestHeight: sizing.foldedFloors.values.map(\.height).min() ?? 0)

private func workspace(_ kinds: [PaneLayoutKind]) throws -> PaneLayoutTree {
  try PaneLayoutFixture.workspace(kinds)
}

/// The frame the tab group's slot leaf was given.
private func slotSize(of result: PaneLayoutFitResult) throws -> PaneLayoutSize {
  let path = try #require(result.plan.panePaths[PaneLayoutTabGroup.slotPaneId])
  return try #require(result.plan.nodes[path]).frame.size
}

private let workspaces: [[PaneLayoutKind]] = [
  [.agent], Array(repeating: .agent, count: 3), Array(repeating: .agent, count: 6),
  [.editor, .agent, .terminal], [.editor, .terminal, .agent, .agent, .web],
  [.screenplay, .agent],
]

/// The window minimum is the narrowest width at which the open rail sits beside any pane at its
/// folded floor: below it the rail would be drawn over a pane.
@Test func theWindowMinimumHoldsTheRailBesideTheWidestPane() {
  let width = window.minimumWindowWidth
  #expect(width == window.largestPaneFootprintWidth + 56 + 2 * PaneLayoutSpacing.outerMargin)
  #expect(!window.railOverlays(windowWidth: width))
  #expect(window.railOverlays(windowWidth: width - 1))
  #expect(window.horizontalInset(windowWidth: width) == PaneLayoutSpacing.outerMargin)
  #expect(
    window.minimumWindowHeight == window.verticalOverhead + sizing.foldedFloors.values.map(\.height)
      .min()!)
}

/// From the minimum up, at any pane count, every pane keeps its whole halo inside the window
/// and the panes never run past the viewport sideways; the rail never covers a pane.
@Test(arguments: workspaces)
func panesFitTheViewportFromTheMinimumUp(kinds: [PaneLayoutKind]) throws {
  let tree = try workspace(kinds)
  let widths = stride(from: window.minimumWindowWidth, through: 1800, by: 41)
  for width in widths {
    #expect(window.horizontalInset(windowWidth: width) >= PaneShellMetrics.halo, "\(width)")
    for railOpen in [false, true] {
      #expect(!window.railOverlays(windowWidth: width), "\(width)")
      for height in [window.minimumWindowHeight, 600, 900] {
        let viewport = window.paneViewport(
          window: PaneLayoutSize(width: width, height: height), railOpen: railOpen)
        let result = try PaneLayoutFit.layout(tree: tree, viewport: viewport, sizing: sizing)
        let root = try #require(result.plan.nodes[PaneLayoutNodePath()])
        #expect(
          !root.overflowAxes.contains(.horizontal),
          "\(kinds.count) panes, \(width) x \(height), rail \(railOpen)")
        for rect in result.plan.paneRects.values {
          #expect(rect.origin.x + rect.size.width <= viewport.width + 0.001)
        }
        if let group = result.plan.tabGroup {
          let slot = try slotSize(of: result)
          #expect(!group.overflowAxes(inSlot: slot).contains(.horizontal))
        }
      }
    }
  }
}

/// Six agent panes in the smallest window are one tab group, and the shown pane scrolls under
/// the strip instead of spilling over it and the top bar.
@Test func sixAgentsAtTheMinimumWindowScrollInsideOneTabGroup() throws {
  let tree = try workspace(Array(repeating: .agent, count: 6))
  let viewport = window.paneViewport(
    window: PaneLayoutSize(width: window.minimumWindowWidth, height: window.minimumWindowHeight),
    railOpen: true)
  let result = try PaneLayoutFit.layout(tree: tree, viewport: viewport, sizing: sizing)
  let group = try #require(result.plan.tabGroup)
  #expect(group.paneIds.count == 6)
  let slot = try slotSize(of: result)
  let strip = PaneLayoutContentMetrics.companionTabStripHeight
  #expect(group.paneArea(inSlot: slot).height == slot.height - strip)
  let agent = try #require(sizing.floors[.agent])
  let overflow = group.overflowAxes(inSlot: slot)
  #expect(overflow.contains(.vertical) == (agent.height > slot.height - strip))
  #expect(!overflow.contains(.horizontal))
  #expect(group.paneRect.size.height == agent.height)
}

/// While the new-pane chooser is open the other panes share a tab group beside it when they
/// do not fit in a row; the chooser is never tabbed.
@Test func theNewPaneChooserStaysBesideATabGroupOfTheOtherPanes() throws {
  let chooser = "chooser"
  let chooserFloor = PaneLayoutFloorFormula.newPaneChooserSurfaceFloor(fonts: fonts)
  let policy = sizing.overridingFloor(
    forPaneId: chooser,
    with: PaneLayoutFloorFormula.footprint(chooserFloor, halo: halo))
  var tree = try workspace([.editor, .terminal])
  tree = try PaneLayoutArrangement.inserting(
    PaneLayoutLeaf(paneId: chooser, kind: .terminal), into: tree, rule: .initialByType,
    sizing: policy)
  let viewport = window.paneViewport(
    window: PaneLayoutSize(width: 1024, height: 700), railOpen: true)
  let result = try PaneLayoutFit.layout(tree: tree, viewport: viewport, sizing: policy)
  #expect(!result.plan.nodes.values.contains { $0.overflowAxes.contains(.horizontal) })
  #expect(result.plan.paneRects[chooser] != nil)
  if let group = result.plan.tabGroup {
    #expect(!group.paneIds.contains(chooser))
  }
}

/// The top bar's first control starts at the same distance from the window's edge at every
/// width, past the traffic lights.
@Test func theTopBarStartsPastTheTrafficLightsAtEveryWidth() {
  let leadingInset = 52.0
  let atStandard = PaneLayoutSpacing.outerMargin + leadingInset
  for width in stride(from: 380.0, through: 1600, by: 37) {
    let fromEdge =
      window.horizontalInset(windowWidth: width)
      + window.topBarLeading(windowWidth: width, leadingInset: leadingInset)
    #expect(fromEdge == atStandard, "\(width)")
  }
}
