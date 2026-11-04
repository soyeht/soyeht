import Testing

@testable import SoyehtClient

@Test func anEdgeFadesOnlyWhereContentIsCutOff() {
  // Chips wider than their row, at rest: more on the trailing side only.
  let start = ScrollEdgeOverflow(contentWidth: 500, containerWidth: 300, offset: 0)
  #expect(!start.hasLeading)
  #expect(start.hasTrailing)
  // Scrolled to the middle: more on both sides.
  let middle = ScrollEdgeOverflow(contentWidth: 500, containerWidth: 300, offset: 100)
  #expect(middle.hasLeading && middle.hasTrailing)
  // Scrolled to the end: more on the leading side only.
  let end = ScrollEdgeOverflow(contentWidth: 500, containerWidth: 300, offset: 200)
  #expect(end.hasLeading)
  #expect(!end.hasTrailing)
  // Chips that fit, or overshoot by a rounding error, fade nowhere.
  let fits = ScrollEdgeOverflow(contentWidth: 280, containerWidth: 300, offset: 0)
  #expect(!fits.hasLeading && !fits.hasTrailing)
  let rounding = ScrollEdgeOverflow(contentWidth: 300.4, containerWidth: 300, offset: 0)
  #expect(!rounding.hasTrailing)
  // A bounce past either end is not content cut off.
  let bounce = ScrollEdgeOverflow(contentWidth: 500, containerWidth: 300, offset: -30)
  #expect(!bounce.hasLeading)
}
