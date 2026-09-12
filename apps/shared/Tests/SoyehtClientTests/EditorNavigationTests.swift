import Testing

@testable import SoyehtClient

@Test func doubleShiftRequiresTwoOrderedPressesInsideTheWindow() {
  var detector = DoubleShiftDetector(maximumInterval: 0.35)

  let first = detector.registerShiftDown(at: 1.0)
  let pair = detector.registerShiftDown(at: 1.30)
  let afterPair = detector.registerShiftDown(at: 2.0)
  let tooLate = detector.registerShiftDown(at: 2.36)
  let nextPair = detector.registerShiftDown(at: 2.60)
  let outOfOrder = detector.registerShiftDown(at: 2.59)

  #expect(!first)
  #expect(pair)
  #expect(!afterPair)
  #expect(!tooLate)
  #expect(nextPair)
  #expect(!outOfOrder)
}
