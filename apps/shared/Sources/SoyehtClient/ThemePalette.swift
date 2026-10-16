import Foundation

public struct SoyehtThemePalette: Equatable, Sendable {
  public let screen: String
  public let canvas: String
  public let face: String
  public let hover: String
  public let well: String
  public let ink: String
  public let secondary: String
  public let muted: String
  public let accent: String
  public let onAccent: String
  public let shadowDark: String
  public let shadowLight: String
  public let wellShadow: String
  public let wellRim: String
  public let wellLip: String?
  public let selection: String
  public let agentPlates: [String]
  public let ansi: [String]
  public let syntaxType: String
  public let diffAdded: String
  public let presentationLetterbox: String

  public var danger: String { ansi[1] }
  public var success: String { ansi[2] }
  public var warning: String { ansi[3] }
  public var isDark: Bool { ThemeLabColorMath.lightness(of: canvas) < 50 }
}

extension SoyehtThemePalette {
  /// The nine palettes are generated from `contracts/design-tokens.json` (`Generated.swift`).
  static func make(
    screen: String, canvas: String, face: String, hover: String, well: String,
    ink: String, secondary: String, muted: String, accent: String, onAccent: String,
    shadowDark: String, shadowLight: String, wellShadow: String, wellRim: String,
    wellLip: String?, selection: String, presentationLetterbox: String,
    agents: [String], ansi: [String],
    syntaxType: String? = nil, diffAdded: String? = nil
  ) -> SoyehtThemePalette {
    SoyehtThemePalette(
      screen: screen, canvas: canvas, face: face, hover: hover, well: well, ink: ink,
      secondary: secondary, muted: muted, accent: accent, onAccent: onAccent,
      shadowDark: shadowDark, shadowLight: shadowLight, wellShadow: wellShadow,
      wellRim: wellRim, wellLip: wellLip, selection: selection, agentPlates: agents, ansi: ansi,
      syntaxType: syntaxType ?? ansi[13], diffAdded: diffAdded ?? ansi[10],
      presentationLetterbox: presentationLetterbox)
  }
}

enum ThemeHexColorMath {
  static func isCanonicalHex(_ value: String) -> Bool {
    guard value.count == 7, value.first == "#" else { return false }
    return value.dropFirst().allSatisfy { $0.isHexDigit && !$0.isLowercase }
  }

  static func rgb8(_ hex: String) -> (UInt8, UInt8, UInt8)? {
    guard isCanonicalHex(hex), let value = UInt32(hex.dropFirst(), radix: 16) else { return nil }
    return (UInt8((value >> 16) & 0xFF), UInt8((value >> 8) & 0xFF), UInt8(value & 0xFF))
  }
}

/// Whether a palette is dark is decided by the CIE lightness of its canvas.
enum ThemeLabColorMath {
  private static let epsilon = 0.008856
  private static let kappa = 903.3

  static func lightness(of hex: String) -> Double {
    guard let rgb = ThemeHexColorMath.rgb8(hex) else { return 0 }
    let luminance = 0.2126 * linear(rgb.0) + 0.7152 * linear(rgb.1) + 0.0722 * linear(rgb.2)
    return 116 * curve(luminance) - 16
  }

  private static func curve(_ value: Double) -> Double {
    value > epsilon ? pow(value, 1.0 / 3) : (kappa * value + 16) / 116
  }

  private static func linear(_ channel: UInt8) -> Double {
    let value = Double(channel) / 255
    return value <= 0.04045 ? value / 12.92 : pow((value + 0.055) / 1.055, 2.4)
  }
}
