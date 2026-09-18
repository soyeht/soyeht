import Testing

@testable import SoyehtClient

private let acceptedFontMetrics = PaneLayoutFontMetrics(
  terminal: PaneLayoutFontMetric(characterWidth: 8, lineHeight: 18, pointSize: 13),
  editor: PaneLayoutFontMetric(characterWidth: 6.9, lineHeight: 19, pointSize: 11.5),
  chrome: PaneLayoutFontMetric(characterWidth: 7.2, lineHeight: 17, pointSize: 12),
  agent: PaneLayoutFontMetric(characterWidth: 9, lineHeight: 21, pointSize: 15),
  prose: PaneLayoutFontMetric(characterWidth: 11.037, lineHeight: 32, pointSize: 20),
  screenplay: PaneLayoutFontMetric(characterWidth: 9.59375, lineHeight: 27, pointSize: 16))

@Test func acceptedMetricsDeriveEveryMeasuredPaneFloor() {
  let surfaces = PaneLayoutFloorFormula.surfaceFloors(fonts: acceptedFontMetrics)
  let footprints = PaneLayoutFloorFormula.footprintFloors(
    fonts: acceptedFontMetrics, halo: 15)

  #expect(surfaces[.terminal] == PaneLayoutSize(width: 340, height: 301))
  #expect(surfaces[.editor] == PaneLayoutSize(width: 333, height: 395))
  #expect(surfaces[.agent] == PaneLayoutSize(width: 340, height: 355))
  #expect(surfaces[.web] == PaneLayoutSize(width: 340, height: 361))
  #expect(surfaces[.text] == PaneLayoutSize(width: 317, height: 380))
  #expect(surfaces[.screenplay] == PaneLayoutSize(width: 344, height: 582))
  #expect(surfaces[.presentation] == PaneLayoutSize(width: 340, height: 302))
  #expect(footprints[.terminal] == PaneLayoutSize(width: 370, height: 331))
  #expect(footprints[.screenplay] == PaneLayoutSize(width: 374, height: 612))
  #expect(footprints[.unknownWriting] == footprints[.screenplay])
}

@Test func terminalFloorChangesWhenTheActiveRendererLineHeightChanges() {
  var metrics = acceptedFontMetrics
  metrics.terminal.lineHeight = 16

  let surface = PaneLayoutFloorFormula.surfaceFloors(fonts: metrics)[.terminal]
  #expect(surface == PaneLayoutSize(width: 340, height: 277))
}

@Test func compactWindowPolicyYieldsDecorationBeforePaneFloor() {
  let policy = PaneLayoutWindowPolicy(
    minimumExercisedViewport: PaneLayoutSize(width: 375, height: 375),
    largestPaneFootprintWidth: 374, standardHorizontalInset: 18,
    topInset: 8, topBarHeight: 42, paneVerticalInset: 17, railWidth: 56)

  #expect(policy.minimumExercisedViewport == PaneLayoutSize(width: 375, height: 375))
  #expect(policy.horizontalInset(windowWidth: 375) == 0.5)
  #expect(policy.horizontalInset(windowWidth: 430) == 18)
  #expect(policy.verticalOverhead == 84)
  #expect(
    PaneLayoutFloorFormula.footprintFloors(fonts: acceptedFontMetrics, halo: 15)[.terminal]!.height
      + policy.verticalOverhead == 415)
  #expect(policy.railOverlays(windowWidth: 430))
  #expect(!policy.railOverlays(windowWidth: 466))
}

@Test func d8KeepsPaneInsetsSymmetricAtBothMeasuredHeights() {
  let policy = PaneLayoutWindowPolicy(
    minimumExercisedViewport: PaneLayoutSize(width: 375, height: 375),
    largestPaneFootprintWidth: 374, standardHorizontalInset: 18,
    topInset: 8, topBarHeight: 42, paneVerticalInset: 17, railWidth: 56)

  #expect(policy.paneVerticalInset == 17)
  #expect(
    policy.paneViewport(window: PaneLayoutSize(width: 375, height: 375), railOpen: false).height
      == 291)
  #expect(
    policy.paneViewport(window: PaneLayoutSize(width: 375, height: 672), railOpen: false).height
      == 588)
}
