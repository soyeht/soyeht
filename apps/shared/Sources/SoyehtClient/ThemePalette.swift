import Foundation

public enum TerminalColorTheme: String, CaseIterable, Codable, Sendable {
  case neoMilk
  case neoPaleMist
  case neoMistyBlue
  case neoSunlitChartreuse
  case neoSunriseGold
  case neoDeepVine
  case neoDeepForest
  case neoMidnightTeal
  case neoDeepHarbor

  public static let seededDefault = TerminalColorTheme.neoMilk

  public var displayName: String {
    switch self {
    case .neoMilk: "Neo · Milk"
    case .neoPaleMist: "Neo · Pale Mist"
    case .neoMistyBlue: "Neo · Misty Blue"
    case .neoSunlitChartreuse: "Neo · Sunlit Chartreuse"
    case .neoSunriseGold: "Neo · Sunrise Gold"
    case .neoDeepVine: "Neo · Deep Vine"
    case .neoDeepForest: "Neo · Deep Forest"
    case .neoMidnightTeal: "Neo · Midnight Teal"
    case .neoDeepHarbor: "Neo · Deep Harbor"
    }
  }

  public var palette: SoyehtThemePalette {
    switch self {
    case .neoMilk: .milk
    case .neoPaleMist: .paleMist
    case .neoMistyBlue: .mistyBlue
    case .neoSunlitChartreuse: .sunlitChartreuse
    case .neoSunriseGold: .sunriseGold
    case .neoDeepVine: .deepVine
    case .neoDeepForest: .deepForest
    case .neoMidnightTeal: .midnightTeal
    case .neoDeepHarbor: .deepHarbor
    }
  }
}

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

  public var allHexValues: [String] {
    [
      screen, canvas, face, hover, well, ink, secondary, muted, accent, onAccent,
      shadowDark, shadowLight, wellShadow, wellRim, selection, syntaxType, diffAdded,
      presentationLetterbox,
    ] + [wellLip].compactMap { $0 } + agentPlates + ansi
  }

  public var isSelfConsistent: Bool {
    ansi.count == 16 && (4...5).contains(agentPlates.count)
      && allHexValues.allSatisfy(ThemeHexColorMath.isCanonicalHex)
  }
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

public enum ThemeHexColorMath {
  public static func isCanonicalHex(_ value: String) -> Bool {
    guard value.count == 7, value.first == "#" else { return false }
    return value.dropFirst().allSatisfy { $0.isHexDigit && !$0.isLowercase }
  }

  public static func mix(_ first: String, _ second: String, amount: Double) -> String? {
    guard let a = rgb8(first), let b = rgb8(second) else { return nil }
    let t = min(1, max(0, amount))
    func blend(_ x: UInt8, _ y: UInt8) -> Int {
      Int((Double(x) * (1 - t) + Double(y) * t).rounded())
    }
    return String(
      format: "#%02X%02X%02X", blend(a.0, b.0), blend(a.1, b.1), blend(a.2, b.2))
  }

  public static func lighten(_ hex: String, by amount: Double) -> String? {
    mix(hex, "#FFFFFF", amount: amount)
  }

  public static func darken(_ hex: String, by amount: Double) -> String? {
    mix(hex, "#000000", amount: amount)
  }

  public static func withAlpha(_ hex: String, _ alpha: Double) -> String? {
    guard let color = rgb8(hex) else { return nil }
    let byte = Int((min(1, max(0, alpha)) * 255).rounded())
    return String(format: "#%02X%02X%02X%02X", color.0, color.1, color.2, byte)
  }

  static func rgb8(_ hex: String) -> (UInt8, UInt8, UInt8)? {
    guard isCanonicalHex(hex), let value = UInt32(hex.dropFirst(), radix: 16) else { return nil }
    return (UInt8((value >> 16) & 0xFF), UInt8((value >> 8) & 0xFF), UInt8(value & 0xFF))
  }
}

public enum ThemeLabColorMath {
  public struct LCh: Equatable, Sendable {
    public var lightness: Double
    public var chroma: Double
    public var hue: Double

    public init(lightness: Double, chroma: Double, hue: Double) {
      self.lightness = lightness
      self.chroma = chroma
      self.hue = hue
    }
  }

  public static func lch(of hex: String) -> LCh? {
    guard let rgb = ThemeHexColorMath.rgb8(hex) else { return nil }
    let value = lab(red: linear(rgb.0), green: linear(rgb.1), blue: linear(rgb.2))
    let chroma = hypot(value.a, value.b)
    var hue = atan2(value.b, value.a) * 180 / .pi
    if hue < 0 { hue += 360 }
    return LCh(lightness: value.lightness, chroma: chroma, hue: hue)
  }

  public static func hex(_ color: LCh) -> String {
    let lightness = min(100, max(0, color.lightness))
    let hue = color.hue.truncatingRemainder(dividingBy: 360)
    let chroma = max(0, color.chroma)
    if let exact = sRGB(lightness: lightness, chroma: chroma, hue: hue) {
      return exact
    }
    var low = 0.0
    var high = chroma
    for _ in 0..<24 {
      let midpoint = (low + high) / 2
      if sRGB(lightness: lightness, chroma: midpoint, hue: hue) == nil {
        high = midpoint
      } else {
        low = midpoint
      }
    }
    return sRGB(lightness: lightness, chroma: low, hue: hue)
      ?? clipped(lightness: lightness, chroma: low, hue: hue)
  }

  public static func lightness(of hex: String) -> Double {
    lch(of: hex)?.lightness ?? 0
  }

  public static func maxChroma(lightness: Double, hue: Double) -> Double {
    lch(of: hex(LCh(lightness: lightness, chroma: 200, hue: hue)))?.chroma ?? 0
  }

  public static func distance(_ first: String, _ second: String) -> Double? {
    guard let a = lch(of: first), let b = lch(of: second) else { return nil }
    let ax = a.chroma * cos(a.hue * .pi / 180)
    let ay = a.chroma * sin(a.hue * .pi / 180)
    let bx = b.chroma * cos(b.hue * .pi / 180)
    let by = b.chroma * sin(b.hue * .pi / 180)
    return sqrt(pow(a.lightness - b.lightness, 2) + pow(ax - bx, 2) + pow(ay - by, 2))
  }

  public static func withLightness(_ hex: String, _ lightness: Double) -> String? {
    guard var color = lch(of: hex) else { return nil }
    color.lightness = lightness
    return self.hex(color)
  }

  public static func contrastRatio(_ first: String, _ second: String) -> Double {
    let a = relativeLuminance(first)
    let b = relativeLuminance(second)
    return (max(a, b) + 0.05) / (min(a, b) + 0.05)
  }

  private static func relativeLuminance(_ hex: String) -> Double {
    guard let rgb = ThemeHexColorMath.rgb8(hex) else { return 0 }
    return 0.2126 * linear(rgb.0) + 0.7152 * linear(rgb.1) + 0.0722 * linear(rgb.2)
  }

  private static func linear(_ channel: UInt8) -> Double {
    let value = Double(channel) / 255
    return value <= 0.04045 ? value / 12.92 : pow((value + 0.055) / 1.055, 2.4)
  }

  private static let whiteX = 0.95047
  private static let whiteZ = 1.08883
  private static let epsilon = 0.008856
  private static let kappa = 903.3

  private static func encode(_ value: Double) -> Double {
    value <= 0.0031308 ? value * 12.92 : 1.055 * pow(value, 1 / 2.4) - 0.055
  }

  private static func lab(red: Double, green: Double, blue: Double) -> (
    lightness: Double, a: Double, b: Double
  ) {
    let x = (0.4124 * red + 0.3576 * green + 0.1805 * blue) / whiteX
    let y = 0.2126 * red + 0.7152 * green + 0.0722 * blue
    let z = (0.0193 * red + 0.1192 * green + 0.9505 * blue) / whiteZ
    func curve(_ value: Double) -> Double {
      value > epsilon ? pow(value, 1.0 / 3) : (kappa * value + 16) / 116
    }
    let fx = curve(x)
    let fy = curve(y)
    let fz = curve(z)
    return (116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz))
  }

  private static func linearRGB(lightness: Double, chroma: Double, hue: Double) -> (
    red: Double, green: Double, blue: Double
  ) {
    let radians = hue * .pi / 180
    let a = chroma * cos(radians)
    let b = chroma * sin(radians)
    let fy = (lightness + 16) / 116
    let fx = fy + a / 500
    let fz = fy - b / 200
    func inverseCurve(_ value: Double) -> Double {
      let cube = value * value * value
      return cube > epsilon ? cube : (116 * value - 16) / kappa
    }
    let x = inverseCurve(fx) * whiteX
    let y = lightness > kappa * epsilon ? pow((lightness + 16) / 116, 3) : lightness / kappa
    let z = inverseCurve(fz) * whiteZ
    return (
      3.2406 * x - 1.5372 * y - 0.4986 * z,
      -0.9689 * x + 1.8758 * y + 0.0415 * z,
      0.0557 * x - 0.2040 * y + 1.0570 * z
    )
  }

  private static func sRGB(lightness: Double, chroma: Double, hue: Double) -> String? {
    let rgb = linearRGB(lightness: lightness, chroma: chroma, hue: hue)
    let tolerance = -0.0001...1.0001
    guard tolerance.contains(rgb.red), tolerance.contains(rgb.green), tolerance.contains(rgb.blue)
    else { return nil }
    return clipped(lightness: lightness, chroma: chroma, hue: hue)
  }

  private static func clipped(lightness: Double, chroma: Double, hue: Double) -> String {
    let rgb = linearRGB(lightness: lightness, chroma: chroma, hue: hue)
    func byte(_ value: Double) -> Int {
      Int((min(1, max(0, encode(min(1, max(0, value))))) * 255).rounded())
    }
    return String(format: "#%02X%02X%02X", byte(rgb.red), byte(rgb.green), byte(rgb.blue))
  }
}

public enum ThemeShadowMath {
  public static func renderRadius(blur: Double) -> Double { blur / 2 }
}
