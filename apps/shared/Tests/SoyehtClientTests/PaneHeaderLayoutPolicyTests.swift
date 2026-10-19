import Testing

@testable import SoyehtClient

let headerPolicy = PaneHeaderLayoutPolicy(
  metrics: PaneHeaderLayoutMetrics(
    horizontalPadding: 24, spacing: 7, stateDotWidth: 7,
    readableTitleWidth: 56, summaryWidth: 72, statusWidth: 84,
    actionWidth: 24))

@Test func agentFloorKeepsReadableTextAndMovesActionsIntoMenu() {
  let plan = headerPolicy.plan(
    availableWidth: 320, actionCount: 6, hasSummary: true, hasStatus: true)

  #expect(plan.actionPlacement == .menu)
  #expect(plan.summaryMaximumWidth == 72)
}

@Test func summaryYieldsBeforeTitleStatusOrActionMenu() {
  let compactWithSummary = headerPolicy.requiredWidth(
    actionCount: 1, includesSummary: true, includesStatus: true)
  let plan = headerPolicy.plan(
    availableWidth: compactWithSummary - 1, actionCount: 6, hasSummary: true,
    hasStatus: true)

  #expect(plan.actionPlacement == .menu)
  #expect(plan.summaryMaximumWidth == nil)
}

@Test func roomyHeadersKeepTheirActionsInline() {
  let threshold = headerPolicy.requiredWidth(
    actionCount: 6, includesSummary: true, includesStatus: true)

  #expect(
    headerPolicy.plan(
      availableWidth: threshold, actionCount: 6, hasSummary: true, hasStatus: true
    ).actionPlacement == .inline)
  #expect(
    headerPolicy.plan(
      availableWidth: threshold - 1, actionCount: 6, hasSummary: true, hasStatus: true
    ).actionPlacement == .menu)
}
