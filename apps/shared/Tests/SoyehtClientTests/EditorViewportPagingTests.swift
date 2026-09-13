import Testing

@testable import SoyehtClient

@Test func smallInitialViewportExpandsToTheBoundedPage() {
  #expect(
    EditorViewportPaging.requestedStart(
      visibleFrom: 0, visibleLines: 40, loadedFrom: 0, loadedTo: 200,
      totalLines: 100_000) == 0)
}

@Test func pagingStaysStillInsideTheLoadedSafetyMargin() {
  #expect(
    EditorViewportPaging.requestedStart(
      visibleFrom: 800, visibleLines: 40, loadedFrom: 0, loadedTo: 2_000,
      totalLines: 100_000) == nil)
}

@Test func pagingReachesTheEndOfAHundredThousandLineFile() {
  let start = EditorViewportPaging.requestedStart(
    visibleFrom: 99_960, visibleLines: 40, loadedFrom: 0, loadedTo: 2_000,
    totalLines: 100_000)
  #expect(start == 98_000)
  #expect(start.map { $0 + EditorViewportPaging.pageLines } == 100_000)
}
