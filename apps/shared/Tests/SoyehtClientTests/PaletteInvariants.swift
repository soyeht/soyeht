import Foundation

@testable import SoyehtClient

/// The checks the palette tests make on the generated palettes: their shape, and the contrast of
/// the roles that sit on each other. They are test-only because nothing in the apps needs them.
extension SoyehtThemePalette {
  var isSelfConsistent: Bool {
    let hexValues =
      [
        screen, canvas, face, hover, well, ink, secondary, muted, accent, onAccent,
        shadowDark, shadowLight, wellShadow, wellRim, selection, syntaxType, diffAdded,
        presentationLetterbox,
      ] + [wellLip].compactMap { $0 } + agentPlates + ansi
    return ansi.count == 16 && (4...5).contains(agentPlates.count)
      && hexValues.allSatisfy(ThemeHexColorMath.isCanonicalHex)
  }
}

/// WCAG contrast ratio of two palette hex colors.
func contrastRatio(_ first: String, _ second: String) -> Double {
  func luminance(_ hex: String) -> Double {
    guard let rgb = ThemeHexColorMath.rgb8(hex) else { return 0 }
    func linear(_ channel: UInt8) -> Double {
      let value = Double(channel) / 255
      return value <= 0.04045 ? value / 12.92 : pow((value + 0.055) / 1.055, 2.4)
    }
    return 0.2126 * linear(rgb.0) + 0.7152 * linear(rgb.1) + 0.0722 * linear(rgb.2)
  }
  let a = luminance(first)
  let b = luminance(second)
  return (max(a, b) + 0.05) / (min(a, b) + 0.05)
}
