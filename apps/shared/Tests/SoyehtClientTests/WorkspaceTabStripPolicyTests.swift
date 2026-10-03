import Testing

@testable import SoyehtClient

private let measuredLongNameWidths: [Double] = [
  216, 204, 216, 151, 177, 216, 216, 197,
  178, 177, 183, 216, 155, 216, 118, 216,
]

private func stripItems(spacing: Double = 10) -> [WorkspaceTabStripItem] {
  var x = 0.0
  return measuredLongNameWidths.enumerated().map { index, width in
    defer { x += width + spacing }
    return WorkspaceTabStripItem(workspaceId: "workspace-\(index)", minX: x, width: width)
  }
}

@Test func measuredSixteenWorkspaceFixtureReportsTheExactHiddenCountAtEveryWidth() {
  let items = stripItems()
  let fixtures = [(375.0, 15), (700, 13), (1_024, 11), (1_440, 10), (3_440, 0)]

  for (width, expectedHiddenCount) in fixtures {
    let plan = WorkspaceTabStripPolicy.plan(
      items: items, activeWorkspaceId: "workspace-0",
      viewport: WorkspaceTabStripViewport(offset: 0, width: width))
    let visibleOrSticky = Set(
      plan.fullyVisibleWorkspaceIds + [plan.sticky?.workspaceId].compactMap { $0 })

    #expect(plan.hiddenCount == expectedHiddenCount)
    #expect(visibleOrSticky.count + plan.hiddenCount == items.count)
    #expect(
      plan.hiddenWorkspaceIds
        == items.map(\.workspaceId).filter { !visibleOrSticky.contains($0) })
  }
}

@Test func oneAndFiveWorkspaceFixturesDoNotLoseAnyExistingWorkspace() {
  let one = Array(stripItems().prefix(1))
  let five = Array(stripItems().prefix(5))
  let wide = WorkspaceTabStripViewport(offset: 0, width: 3_440)
  let compact = WorkspaceTabStripViewport(offset: 0, width: 700)

  #expect(
    WorkspaceTabStripPolicy.plan(
      items: one, activeWorkspaceId: "workspace-0", viewport: compact
    ).hiddenCount == 0)
  #expect(
    WorkspaceTabStripPolicy.plan(
      items: five, activeWorkspaceId: "workspace-0", viewport: compact
    ).hiddenWorkspaceIds == ["workspace-3", "workspace-4"])
  #expect(
    WorkspaceTabStripPolicy.plan(
      items: five, activeWorkspaceId: "workspace-0", viewport: wide
    ).hiddenCount == 0)
}

@Test func activeTabBecomesAStickyProxyAtEitherViewportEdgeAndLeavesTheHiddenCount() {
  let items = stripItems()
  let middleViewport = WorkspaceTabStripViewport(offset: 700, width: 375)

  let leading = WorkspaceTabStripPolicy.plan(
    items: items, activeWorkspaceId: "workspace-0", viewport: middleViewport)
  let trailing = WorkspaceTabStripPolicy.plan(
    items: items, activeWorkspaceId: "workspace-15", viewport: middleViewport)

  #expect(
    leading.sticky == WorkspaceTabStickyPresentation(workspaceId: "workspace-0", edge: .leading))
  #expect(leading.hiddenCount == 14)
  #expect(!leading.hiddenWorkspaceIds.contains("workspace-0"))
  #expect(
    trailing.sticky == WorkspaceTabStickyPresentation(workspaceId: "workspace-15", edge: .trailing))
  #expect(trailing.hiddenCount == 14)
  #expect(!trailing.hiddenWorkspaceIds.contains("workspace-15"))
}

@Test func stickyProxyConsumesViewportAndCoveredCellsCountAsHidden() {
  let items = [
    WorkspaceTabStripItem(workspaceId: "active", minX: -170, width: 160),
    WorkspaceTabStripItem(workspaceId: "covered", minX: 20, width: 170),
    WorkspaceTabStripItem(workspaceId: "visible", minX: 230, width: 170),
    WorkspaceTabStripItem(workspaceId: "trailing", minX: 430, width: 170),
  ]

  let plan = WorkspaceTabStripPolicy.plan(
    items: items, activeWorkspaceId: "active",
    viewport: WorkspaceTabStripViewport(offset: 0, width: 500),
    stickyOcclusionWidth: 220)

  #expect(plan.sticky == WorkspaceTabStickyPresentation(workspaceId: "active", edge: .leading))
  #expect(plan.fullyVisibleWorkspaceIds == ["visible"])
  #expect(plan.hiddenWorkspaceIds == ["covered", "trailing"])
  #expect(plan.hiddenCount == 2)
}

@Test func trailingStickyProxyConsumesViewportFromItsOwnEdge() {
  let items = [
    WorkspaceTabStripItem(workspaceId: "leading", minX: -80, width: 170),
    WorkspaceTabStripItem(workspaceId: "visible", minX: 100, width: 170),
    WorkspaceTabStripItem(workspaceId: "covered", minX: 310, width: 170),
    WorkspaceTabStripItem(workspaceId: "active", minX: 520, width: 170),
  ]

  let plan = WorkspaceTabStripPolicy.plan(
    items: items, activeWorkspaceId: "active",
    viewport: WorkspaceTabStripViewport(offset: 0, width: 500),
    stickyOcclusionWidth: 220)

  #expect(plan.sticky == WorkspaceTabStickyPresentation(workspaceId: "active", edge: .trailing))
  #expect(plan.fullyVisibleWorkspaceIds == ["visible"])
  #expect(plan.hiddenWorkspaceIds == ["leading", "covered"])
  #expect(plan.hiddenCount == 2)
}

@Test func partiallyClippedActiveTabUsesStickyUntilItsCanonicalCellIsWhole() {
  let items = [WorkspaceTabStripItem(workspaceId: "active", minX: 40, width: 200)]

  let clipped = WorkspaceTabStripPolicy.plan(
    items: items, activeWorkspaceId: "active",
    viewport: WorkspaceTabStripViewport(offset: 50, width: 200))
  let whole = WorkspaceTabStripPolicy.plan(
    items: items, activeWorkspaceId: "active",
    viewport: WorkspaceTabStripViewport(offset: 40, width: 200))

  #expect(clipped.sticky?.edge == .leading)
  #expect(whole.sticky == nil)
  #expect(whole.fullyVisibleWorkspaceIds == ["active"])
}

@Test func revealingAStickyCellUsesTheSmallestClampedOffset() {
  let items = stripItems()
  let contentWidth = items.last!.maxX
  let viewport = WorkspaceTabStripViewport(offset: 700, width: 375)

  #expect(
    WorkspaceTabStripPolicy.revealOffset(
      workspaceId: "workspace-0", items: items, viewport: viewport,
      contentWidth: contentWidth) == 0)
  #expect(
    WorkspaceTabStripPolicy.revealOffset(
      workspaceId: "workspace-15", items: items, viewport: viewport,
      contentWidth: contentWidth) == contentWidth - viewport.width)
}

@Test func closingTheActiveWorkspaceSelectsNextThenPreviousAtTheEnd() {
  let order = ["first", "middle", "last"]

  #expect(
    WorkspaceTabStripPolicy.selectionAfterClosing(
      workspaceId: "first", orderedWorkspaceIds: order) == "middle")
  #expect(
    WorkspaceTabStripPolicy.selectionAfterClosing(
      workspaceId: "middle", orderedWorkspaceIds: order) == "last")
  #expect(
    WorkspaceTabStripPolicy.selectionAfterClosing(
      workspaceId: "last", orderedWorkspaceIds: order) == "middle")
  #expect(
    WorkspaceTabStripPolicy.selectionAfterClosing(
      workspaceId: "only", orderedWorkspaceIds: ["only"]) == nil)
}

@Test func keyboardFocusFollowsCanonicalOrderWithoutWrappingOrReordering() {
  let order = ["first", "middle", "last"]

  #expect(
    WorkspaceTabStripPolicy.focusNeighbor(
      of: "middle", direction: .previous, orderedWorkspaceIds: order) == "first")
  #expect(
    WorkspaceTabStripPolicy.focusNeighbor(
      of: "middle", direction: .next, orderedWorkspaceIds: order) == "last")
  #expect(
    WorkspaceTabStripPolicy.focusNeighbor(
      of: "first", direction: .previous, orderedWorkspaceIds: order) == nil)
  #expect(
    WorkspaceTabStripPolicy.focusNeighbor(
      of: "last", direction: .next, orderedWorkspaceIds: order) == nil)
  #expect(order == ["first", "middle", "last"])
}

@Test func selectingAnyWorkspaceScrollsItsTabFullyIntoView() {
  let items = stripItems()
  let contentWidth = items.last!.maxX
  for start in [0.0, 700, contentWidth - 375] {
    let viewport = WorkspaceTabStripViewport(offset: start, width: 375)
    for item in items {
      let offset =
        WorkspaceTabStripPolicy.revealOffset(
          workspaceId: item.workspaceId, items: items, viewport: viewport,
          contentWidth: contentWidth) ?? viewport.offset
      let revealed = WorkspaceTabStripViewport(offset: offset, width: viewport.width)
      let plan = WorkspaceTabStripPolicy.plan(
        items: items, activeWorkspaceId: item.workspaceId, viewport: revealed)

      #expect(plan.fullyVisibleWorkspaceIds.contains(item.workspaceId))
      #expect(plan.sticky == nil)
      #expect(offset >= 0 && offset <= contentWidth - viewport.width)
    }
  }
}

@Test func alreadyVisibleActiveTabDoesNotScroll() {
  let items = stripItems()
  let viewport = WorkspaceTabStripViewport(offset: 0, width: 700)
  #expect(
    WorkspaceTabStripPolicy.revealOffset(
      workspaceId: "workspace-1", items: items, viewport: viewport,
      contentWidth: items.last!.maxX) == nil)
}

@Test func overflowDetectionReportsEachHiddenSide() {
  let contentWidth = stripItems().last!.maxX
  let atStart = WorkspaceTabStripPolicy.overflow(
    contentWidth: contentWidth, viewport: WorkspaceTabStripViewport(offset: 0, width: 700))
  #expect(atStart == WorkspaceTabStripOverflow(hiddenLeading: false, hiddenTrailing: true))

  let middle = WorkspaceTabStripPolicy.overflow(
    contentWidth: contentWidth, viewport: WorkspaceTabStripViewport(offset: 700, width: 700))
  #expect(middle == WorkspaceTabStripOverflow(hiddenLeading: true, hiddenTrailing: true))

  let atEnd = WorkspaceTabStripPolicy.overflow(
    contentWidth: contentWidth,
    viewport: WorkspaceTabStripViewport(offset: contentWidth - 700, width: 700))
  #expect(atEnd == WorkspaceTabStripOverflow(hiddenLeading: true, hiddenTrailing: false))

  let fits = WorkspaceTabStripPolicy.overflow(
    contentWidth: contentWidth, viewport: WorkspaceTabStripViewport(offset: 0, width: 3_440))
  #expect(!fits.overflows)
  // Layout rounding below half a point never reports a hidden edge.
  let rounded = WorkspaceTabStripPolicy.overflow(
    contentWidth: 700.4, viewport: WorkspaceTabStripViewport(offset: 0.3, width: 700))
  #expect(!rounded.overflows)

  #expect(WorkspaceTabStripPolicy.overflows(contentWidth: contentWidth, availableWidth: 1_024))
  #expect(!WorkspaceTabStripPolicy.overflows(contentWidth: 700.4, availableWidth: 700))
}

@Test func edgeChevronsPageMostOfAViewportAndStopAtTheEnds() {
  let contentWidth = 2_000.0
  let viewport = WorkspaceTabStripViewport(offset: 500, width: 500)
  #expect(
    WorkspaceTabStripPolicy.pageOffset(
      toward: .trailing, viewport: viewport, contentWidth: contentWidth) == 900)
  #expect(
    WorkspaceTabStripPolicy.pageOffset(
      toward: .leading, viewport: viewport, contentWidth: contentWidth) == 100)
  #expect(
    WorkspaceTabStripPolicy.pageOffset(
      toward: .leading, viewport: WorkspaceTabStripViewport(offset: 100, width: 500),
      contentWidth: contentWidth) == 0)
  #expect(
    WorkspaceTabStripPolicy.pageOffset(
      toward: .trailing, viewport: WorkspaceTabStripViewport(offset: 1_400, width: 500),
      contentWidth: contentWidth) == 1_500)
}

@Test func onlyAVerticalMouseWheelTurnScrollsTheStripSideways() {
  #expect(
    WorkspaceTabStripPolicy.wheelScrollsHorizontally(
      deltaX: 0, deltaY: -3, hasPreciseDeltas: false))
  #expect(
    !WorkspaceTabStripPolicy.wheelScrollsHorizontally(
      deltaX: -3, deltaY: 0, hasPreciseDeltas: false))
  #expect(
    !WorkspaceTabStripPolicy.wheelScrollsHorizontally(
      deltaX: 0, deltaY: -12, hasPreciseDeltas: true))
}
