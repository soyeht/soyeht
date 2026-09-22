import Testing

@testable import SoyehtClient

@Test func slideShowMapsNavigationInputsWithoutCapturingUnrelatedCommands() {
  let policy = WritingSlideShowPolicy.standard

  #expect(policy.command(for: .rightArrow) == .advance)
  #expect(policy.command(for: .pageDown) == .advance)
  #expect(policy.command(for: .space) == .advance)
  #expect(policy.command(for: .click) == .advance)
  #expect(policy.command(for: .leftArrow) == .previous)
  #expect(policy.command(for: .pageUp) == .previous)
  #expect(policy.command(for: .escape) == .exit)
  #expect(policy.command(for: .other) == nil)
  #expect(policy.command(for: .rightArrow, commandModified: true) == nil)
  #expect(policy.command(for: .other, commandModified: true) == nil)
}

@Test func slideShowStageIsCenteredSixteenByNineAtEveryViewportShape() {
  let policy = WritingSlideShowPolicy.standard

  let wide = policy.fittedStage(in: PaneLayoutSize(width: 1_920, height: 900))
  #expect(wide.size == PaneLayoutSize(width: 1_600, height: 900))
  #expect(wide.origin == PaneLayoutPoint(x: 160, y: 0))

  let tall = policy.fittedStage(in: PaneLayoutSize(width: 900, height: 1_200))
  #expect(tall.size == PaneLayoutSize(width: 900, height: 506.25))
  #expect(tall.origin == PaneLayoutPoint(x: 0, y: 346.875))

  #expect(
    policy.fittedStage(in: PaneLayoutSize(width: 0, height: 900)).size
      == PaneLayoutSize(width: 0, height: 0))
}

@Test func slideShowNavigationClampsAndReconcilesWhenSlidesChange() {
  var state = WritingSlideShowState(index: 2, slideCount: 3)

  let advancedShouldExit = state.apply(.advance)
  #expect(!advancedShouldExit)
  #expect(state.index == 2)
  let previousShouldExit = state.apply(.previous)
  #expect(!previousShouldExit)
  #expect(state.index == 1)
  let oneSlideShouldExit = state.reconcile(slideCount: 1)
  #expect(!oneSlideShouldExit)
  #expect(state.index == 0)
  let emptyShouldExit = state.reconcile(slideCount: 0)
  #expect(emptyShouldExit)
  #expect(state.index == 0)
}

@Test func slideShowCursorBalancesEveryHideWithAShow() {
  var cursor = WritingSlideShowCursorState()

  let firstIdle = cursor.reduce(.idleExpired)
  let secondIdle = cursor.reduce(.idleExpired)
  let firstActivity = cursor.reduce(.activity)
  let secondActivity = cursor.reduce(.activity)
  let thirdIdle = cursor.reduce(.idleExpired)
  let end = cursor.reduce(.sessionEnded)
  #expect(firstIdle == .hide)
  #expect(secondIdle == .none)
  #expect(firstActivity == .show)
  #expect(secondActivity == .none)
  #expect(thirdIdle == .hide)
  #expect(end == .show)
  #expect(!cursor.isHidden)
}

@Test func slideShowLeavesAnAlreadyFullScreenWindowUnchanged() {
  var window = WritingSlideShowWindowState()

  let begin = window.begin(isFullScreen: true)
  #expect(begin == .activate)
  #expect(window.phase == .active)
  let exit = window.requestExit()
  #expect(exit == .finish)
  #expect(window.phase == .inactive)
}

@Test func slideShowQueuesExitDuringNativeFullScreenTransition() {
  var window = WritingSlideShowWindowState()

  let begin = window.begin(isFullScreen: false)
  let queuedExit = window.requestExit()
  let entered = window.didEnterFullScreen()
  #expect(begin == .enterFullScreen)
  #expect(queuedExit == .none)
  #expect(entered == .exitFullScreen)
  #expect(window.phase == .exitingFullScreen)
  let exited = window.didExitFullScreen()
  #expect(exited == .finish)
  #expect(window.phase == .inactive)
}

@Test func slideShowFinishesCleanlyWhenFullScreenEntryFails() {
  var window = WritingSlideShowWindowState()

  let begin = window.begin(isFullScreen: false)
  let failed = window.didFailToEnterFullScreen()
  #expect(begin == .enterFullScreen)
  #expect(failed == .finish)
  #expect(window.phase == .inactive)
}
