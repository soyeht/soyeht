#if os(macOS)
  import AppKit
  import SwiftUI
  import Testing

  @testable import SoyehtClient

  /// The gap between two pane cards holds a whole halo: the shadow of one card never reaches
  /// the edge of its neighbour, whatever the theme. The halo is `shadowOffset + shadowBlur` and
  /// comes from `design-tokens.json`, as does the gap.
  @Test func theGapBetweenCardsHoldsAWholeHalo() {
    #expect(PaneLayoutSpacing.paneGap >= PaneShellMetrics.halo)
    #expect(PaneLayoutSpacing.gaps.horizontal >= PaneShellMetrics.halo)
    #expect(PaneLayoutSpacing.gaps.vertical >= PaneShellMetrics.halo)
  }

  /// A pane whose chrome is wider than its card is cut at the card's edge: it never paints over
  /// the neighbouring card or into the gap between them.
  @MainActor @Test(arguments: [TerminalColorTheme.neoMilk, .neoMidnightTeal])
  func contentThatOverflowsACardIsCutAtItsEdge(theme: TerminalColorTheme) throws {
    let colors = SoyehtThemeColorSet(palette: theme.palette)
    let card = CGSize(width: 200, height: 160)
    let gap = PaneLayoutSpacing.paneGap
    let margin = PaneLayoutSpacing.outerMargin
    let size = CGSize(
      width: 2 * card.width + gap + 2 * margin, height: card.height + 2 * margin)
    // The right card is drawn last, so its overflow to the left would show over the left card.
    let overflowing = Color(red: 1, green: 0, blue: 0)
      .frame(width: 3 * card.width, height: card.height / 2)
      .frame(width: card.width, height: card.height, alignment: .trailing)
      .paneShell(colors)
    let view = HStack(spacing: gap) {
      Color.clear.frame(width: card.width, height: card.height).paneShell(colors)
      overflowing
    }
    .frame(width: size.width, height: size.height)
    .background(colors.canvas)
    let bitmap = try #require(PaneInteriorRendering.bitmap(view, size: size))
    let y = margin + card.height / 2
    for x in stride(from: margin, to: margin + card.width + gap, by: 4) {
      let (red, green, blue) = pixel(bitmap, x: x, y: y)
      #expect(
        !(red > 200 && green < 80 && blue < 80), "\(theme): overflow painted at x = \(x)")
    }
    let (red, green, blue) = pixel(bitmap, x: margin + card.width + gap + 40, y: y)
    #expect(red > 200 && green < 80 && blue < 80, "the card's own content is still drawn")
  }

  private func pixel(_ bitmap: NSBitmapImageRep, x: CGFloat, y: CGFloat) -> (Int, Int, Int) {
    let px = min(bitmap.pixelsWide - 1, max(0, Int(x * 2)))
    let py = min(bitmap.pixelsHigh - 1, max(0, Int(y * 2)))
    guard let data = bitmap.bitmapData else { return (0, 0, 0) }
    let p = data + py * bitmap.bytesPerRow + px * (bitmap.bitsPerPixel / 8)
    return (Int(p[0]), Int(p[1]), Int(p[2]))
  }
#endif
