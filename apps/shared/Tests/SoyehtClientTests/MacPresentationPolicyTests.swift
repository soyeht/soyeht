import Testing

@testable import SoyehtClient

private let sheetPolicy = MacSheetLayoutPolicy()

@Test func everyFixedMacSheetFitsInsideA375PointWindow() {
  let available = PaneLayoutSize(width: 375, height: 812)
  let usableWidth = available.width - 2 * MacSheetLayoutMetrics.standard.windowInset
  let usableHeight = available.height - 2 * MacSheetLayoutMetrics.standard.windowInset

  for kind in MacSheetKind.allCases {
    let plan = sheetPolicy.plan(for: kind, available: available)
    #expect(plan.width <= usableWidth)
    if let height = plan.height { #expect(height <= usableHeight) }
  }
}

@Test func roomyMacSheetsKeepTheirInventoriedSizes() {
  let available = PaneLayoutSize(width: 1600, height: 1000)

  for kind in MacSheetKind.allCases {
    let plan = sheetPolicy.plan(for: kind, available: available)
    let preferred = sheetPolicy.preferredSize(for: kind)
    #expect(plan.width == preferred.width)
    #expect(plan.height == preferred.height)
  }
}

@Test func missingWindowMeasurementUsesTheInventoriedSizeWithoutInventingAViewport() {
  for kind in MacSheetKind.allCases {
    let plan = sheetPolicy.plan(for: kind, available: nil)
    let preferred = sheetPolicy.preferredSize(for: kind)
    #expect(plan.width == preferred.width)
    #expect(plan.height == preferred.height)
  }
}

@Test func compactPreferencesStackInsteadOfKeepingTheWideSidebar() {
  #expect(
    sheetPolicy.plan(
      for: .preferences, available: PaneLayoutSize(width: 375, height: 812)
    ).navigation == .stack)
  #expect(
    sheetPolicy.plan(
      for: .preferences, available: PaneLayoutSize(width: 1200, height: 800)
    ).navigation == .sidebar)
}

@Test func ideaBoardHeaderRemovesOnlyActionTitlesAtCompactWidth() {
  let policy = WritingBeatBoardHeaderPolicy()

  #expect(!policy.plan(viewportWidth: 375).showsActionTitles)
  #expect(policy.plan(viewportWidth: 520).showsActionTitles)
}
