import Testing

@testable import SoyehtClient

/// The order a narrowing header gives things up: the actions fold into the menu, then the
/// activity line goes, then the status word; the title and the menu stay.
@Test func aNarrowingHeaderGivesUpActionsThenActivityThenStatus() {
  func plan(_ width: Double) -> PaneHeaderLayoutPlan {
    headerPolicy.plan(availableWidth: width, actionCount: 6, hasSummary: true, hasStatus: true)
  }
  let inline = headerPolicy.requiredWidth(
    actionCount: 6, includesSummary: true, includesStatus: true)
  let compactWithSummary = headerPolicy.requiredWidth(
    actionCount: 1, includesSummary: true, includesStatus: true)
  let compact = headerPolicy.requiredWidth(
    actionCount: 1, includesSummary: false, includesStatus: true)

  #expect(plan(inline) == PaneHeaderLayoutPlan(actionPlacement: .inline, summaryMaximumWidth: nil))
  #expect(plan(inline - 1).actionPlacement == .menu)
  #expect(plan(inline - 1).summaryMaximumWidth == headerPolicy.metrics.summaryWidth)
  #expect(plan(compactWithSummary).summaryMaximumWidth == headerPolicy.metrics.summaryWidth)
  #expect(plan(compactWithSummary - 1).summaryMaximumWidth == nil)
  #expect(plan(compactWithSummary - 1).showsStatus)
  #expect(plan(compact).showsStatus)
  #expect(!plan(compact - 1).showsStatus)
  #expect(plan(0).actionPlacement == .menu)
}

/// The header reserves room for the widest status word, so the word is never truncated.
@Test func theWidestStatusWordIsWaitingForYou() {
  #expect(AgentPaneStatusPresentation.longestTitle == "waiting for you")
}
