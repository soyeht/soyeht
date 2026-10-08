#if os(macOS)
  import CoreGraphics
  import Foundation
  import SwiftUI
  import Testing

  @testable import SoyehtClient

  /// The mark is the app icon and the logo in the interface: its proportions are those of the
  /// first iOS app's welcome screen, and the icon generator scales them, so they stay pinned.
  @Test func markKeepsTheWelcomeScreenProportions() {
    #expect(SoyehtMarkMetrics.faceSide == 96)
    #expect(SoyehtMarkMetrics.cornerRadius == 20)
    #expect(SoyehtMarkMetrics.glyphSize == 34)
    #expect(SoyehtMarkMetrics.elevationOffset == 12)
    #expect(SoyehtMarkMetrics.elevationBlur == 24)
    #expect(abs(SoyehtMarkMetrics.glyphProportion - 34.0 / 96.0) < 1e-9)
  }

  /// The face and the chevron are drawn in the palette's roles, never in a colour of their own.
  @MainActor @Test(arguments: [TerminalColorTheme.neoMilk, .neoMidnightTeal])
  func markDrawsFaceAndChevronInPaletteRoles(theme: TerminalColorTheme) throws {
    let colors = SoyehtThemeColorSet(palette: theme.palette)
    let side = SoyehtMarkMetrics.faceSide
    let scale: CGFloat = 4
    let renderer = ImageRenderer(
      content: SoyehtMark(colors: colors, side: side).frame(width: side, height: side))
    renderer.scale = scale
    let image = try #require(renderer.cgImage)
    // Read the pixels in sRGB, the space the palette's hex values are written in.
    var pixels = [UInt8](repeating: 0, count: image.width * image.height * 4)
    let context = try #require(
      CGContext(
        data: &pixels, width: image.width, height: image.height, bitsPerComponent: 8,
        bytesPerRow: image.width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
    context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))

    func hex(_ x: CGFloat, _ y: CGFloat) -> String {
      let offset = (Int(y * scale) * image.width + Int(x * scale)) * 4
      return String(format: "#%02X%02X%02X", pixels[offset], pixels[offset + 1], pixels[offset + 2])
    }

    #expect(hex(side * 0.25, side / 2) == theme.palette.face)
    // The chevron's vertex sits on the centre line, right of the centre.
    #expect(hex(side / 2 + side * 0.1, side / 2) == theme.palette.accent)
  }
#endif
