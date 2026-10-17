import Testing

@testable import SoyehtClient

/// The Mac window policy with the production spacing and the 56 pt rail.
private let window = PaneLayoutWindowPolicy(
  minimumExercisedViewport: PaneLayoutSize(width: 375, height: 375),
  largestPaneFootprintWidth: 374, standardHorizontalInset: PaneLayoutSpacing.outerMargin,
  topInset: 8, topBarHeight: 42, paneVerticalInset: PaneLayoutSpacing.outerMargin,
  railWidth: 56)

@Test(arguments: [1440.0, 1024.0])
func railClosedMarginsMatchOnBothSides(width: Double) {
  // Tela 1: 18 pt screen padding plus 6 pt grid padding on each side.
  let placement = window.horizontalPlacement(windowWidth: width, railOpen: false)
  #expect(placement.paneLeading == 24)
  #expect(placement.paneTrailing == 24)
  let viewport = window.paneViewport(
    window: PaneLayoutSize(width: width, height: 900), railOpen: false)
  #expect(placement.paneLeading + viewport.width + placement.paneTrailing == width)
}

@Test(arguments: [1440.0, 1024.0])
func railOpenPlacementMatchesTheApprovedFrame(width: Double) {
  // Tela 12: the rail starts at the 18 pt screen padding and ends at 74 pt; the grid's 6 pt
  // padding puts the first card at 80 pt. The right edge keeps 24 pt, as with the rail closed.
  let placement = window.horizontalPlacement(windowWidth: width, railOpen: true)
  #expect(placement.railLeading == PaneLayoutSpacing.screenPadding)
  #expect(placement.paneLeading == 80)
  #expect(placement.paneLeading - (placement.railLeading + 56) == PaneLayoutSpacing.gridPadding)
  #expect(placement.paneTrailing == 24)
  let viewport = window.paneViewport(
    window: PaneLayoutSize(width: width, height: 900), railOpen: true)
  #expect(placement.paneLeading + viewport.width + placement.paneTrailing == width)
}

@Test func narrowWindowShrinksTheInsetsAndOverlaysTheRail() {
  let placement = window.horizontalPlacement(windowWidth: 430, railOpen: true)
  #expect(window.railOverlays(windowWidth: 430))
  #expect(placement.paneLeading == placement.paneTrailing)
  #expect(placement.railLeading == max(0, placement.paneTrailing - PaneLayoutSpacing.gridPadding))
}

@Test func aPageClearsTheOverlaidRailAtTheNarrowestWindow() {
  let width = 375.0
  let placement = window.horizontalPlacement(windowWidth: width, railOpen: true)
  let clearance = window.railClearance(windowWidth: width, railOpen: true)
  #expect(placement.railOverlays)
  #expect(placement.paneLeading + clearance >= placement.railLeading + 56)
  #expect(
    placement.paneLeading + clearance
      == placement.railLeading + 56 + PaneLayoutSpacing.gridPadding)
  let viewport = window.paneViewport(
    window: PaneLayoutSize(width: width, height: 375), railOpen: true)
  #expect(viewport.width - clearance > 0)
}

@Test(arguments: [1440.0, 1024.0])
func aPageNeedsNoClearanceWhenTheRailIsClosedOrBesideThePanes(width: Double) {
  #expect(window.railClearance(windowWidth: width, railOpen: true) == 0)
  #expect(window.railClearance(windowWidth: width, railOpen: false) == 0)
  #expect(window.railClearance(windowWidth: 375, railOpen: false) == 0)
}
