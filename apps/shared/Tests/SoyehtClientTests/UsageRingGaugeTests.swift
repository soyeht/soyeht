import Foundation
import Testing

@testable import SoyehtClient

private let macRing = UsageRingGeometry(
  arcWidth: DesignTokens.UsageRing.arcWidth, trackWidth: DesignTokens.UsageRing.trackWidth)

@Test func theWidestStrokeEndsOnTheBoundsNotBeyondThem() {
  let diameter = DesignTokens.UsageRing.diameter
  let widest = max(macRing.arcWidth, macRing.trackWidth)
  #expect(macRing.outerRadius(width: widest, diameter: diameter) == diameter / 2)
  #expect(macRing.outerRadius(width: macRing.arcWidth, diameter: diameter) <= diameter / 2)
  #expect(macRing.outerRadius(width: macRing.trackWidth, diameter: diameter) <= diameter / 2)
}

@Test func theArcSitsInsideTheTrack() {
  let diameter = DesignTokens.UsageRing.diameter
  #expect(
    macRing.outerRadius(width: macRing.arcWidth, diameter: diameter)
      <= macRing.outerRadius(width: macRing.trackWidth, diameter: diameter))
  #expect(
    macRing.innerRadius(width: macRing.arcWidth, diameter: diameter)
      >= macRing.innerRadius(width: macRing.trackWidth, diameter: diameter))
}

@Test func theGlyphFitsInsideTheInnerEdgeOfTheTrack() {
  let inner = macRing.innerRadius(
    width: macRing.trackWidth, diameter: DesignTokens.UsageRing.diameter)
  // The glyph is a square: its corner is the farthest point from the centre.
  let corner = DesignTokens.UsageRing.glyphSize / 2 * 2.0.squareRoot()
  #expect(corner <= inner)
}

@Test func anArcWiderThanTheTrackStillStaysOnTheBounds() {
  let wide = UsageRingGeometry(arcWidth: 9, trackWidth: 4)
  #expect(wide.pathInset == 4.5)
  #expect(wide.outerRadius(width: 9, diameter: 58) == 29)
  #expect(wide.outerRadius(width: 4, diameter: 58) < 29)
}

@Test func theRingFitsTheRailColumnItIsScrolledIn() {
  // The rail scrolls its ring column, and a scroll view clips at the column. The ring may not
  // need more than the rail leaves after its horizontal padding.
  let column = DesignTokens.Layout.railWidth - 2 * DesignTokens.Space.x8
  #expect(DesignTokens.UsageRing.diameter <= column)
}
