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

  public var danger: String { ansi[1] }
  public var success: String { ansi[2] }
  public var warning: String { ansi[3] }
  public var isDark: Bool { ThemeLabColorMath.lightness(of: canvas) < 50 }

  public var allHexValues: [String] {
    [
      screen, canvas, face, hover, well, ink, secondary, muted, accent, onAccent,
      shadowDark, shadowLight, wellShadow, wellRim, selection, syntaxType, diffAdded,
    ] + [wellLip].compactMap { $0 } + agentPlates + ansi
  }

  public var isSelfConsistent: Bool {
    ansi.count == 16 && (4...5).contains(agentPlates.count)
      && allHexValues.allSatisfy(ThemeHexColorMath.isCanonicalHex)
  }
}

extension SoyehtThemePalette {
  fileprivate static let milk = make(
    screen: "#E8EDF4", canvas: "#E0E5EC", face: "#E8EDF4", hover: "#DCE2EA",
    well: "#D8DEE8", ink: "#3E4A66", secondary: "#6E7A96", muted: "#8E9AB3",
    accent: "#5B7CFA", onAccent: "#FFFFFF", shadowDark: "#A6B4C8",
    shadowLight: "#FFFFFF", wellShadow: "#A6B4C8", wellRim: "#FFFFFF", wellLip: nil,
    selection: "#C9D6EE",
    agents: ["#D8E0FE", "#CDE7DD", "#F6D6DB", "#EEE3CD"],
    ansi: [
      "#1F2633", "#D9556A", "#2E9A73", "#B98A2E", "#3D7DD9", "#9A6BD0",
      "#2E93A6", "#8E9AB3", "#6E7A96", "#ED6F86", "#3FB68B", "#E9B04E",
      "#5B9DF5", "#B08CFF", "#3FB6C8", "#263043",
    ], syntaxType: "#8E5BFA", diffAdded: "#3F9C6A")

  fileprivate static let paleMist = make(
    screen: "#C0C8CA", canvas: "#B8C0C2", face: "#C0C8CA", hover: "#B2BABC",
    well: "#AFB7B9", ink: "#25363B", secondary: "#4B6268", muted: "#5E8089",
    accent: "#0F64D1", onAccent: "#FFFFFF", shadowDark: "#8C9395",
    shadowLight: "#D1D9DB", wellShadow: "#8C9395", wellRim: "#D1D9DB", wellLip: nil,
    selection: "#99B2CC",
    agents: ["#C3A2D1", "#E19BA9", "#94B88A", "#68BCB3", "#91AAC4"],
    ansi: [
      "#1F2F33", "#CE2E48", "#257C5C", "#8B6823", "#286CCD", "#8952C8",
      "#257887", "#4B6268", "#5E8089", "#D71B3D", "#2B7B5E", "#946412",
      "#0E68E2", "#7E42FF", "#267885", "#25363B",
    ])

  fileprivate static let mistyBlue = make(
    screen: "#83A7B4", canvas: "#7B9FAC", face: "#83A7B4", hover: "#769AA7",
    well: "#7296A2", ink: "#0C1012", secondary: "#37464C", muted: "#4A626B",
    accent: "#004497", onAccent: "#FFFFFF", shadowDark: "#517480",
    shadowLight: "#93B7C5", wellShadow: "#517480", wellRim: "#93B7C5", wellLip: nil,
    selection: "#6691AE",
    agents: ["#9E7FAB", "#B97785", "#719368", "#41978F", "#5E89A6"],
    ansi: [
      "#1F2E33", "#9C2336", "#1C5D46", "#694E1A", "#1E519B", "#6B35A8",
      "#1C5A66", "#37464C", "#4A626B", "#A3142F", "#205C47", "#6F4C0E",
      "#0A4FAA", "#4E00FB", "#1D5A64", "#0C1012",
    ])

  fileprivate static let sunlitChartreuse = make(
    screen: "#DCD870", canvas: "#D2CE67", face: "#DCD870", hover: "#CDCA63",
    well: "#C6C35C", ink: "#3E3D28", secondary: "#6A684C", muted: "#88865E",
    accent: "#B55C03", onAccent: "#FFFFFF", shadowDark: "#A2A13C",
    shadowLight: "#ECE77F", wellShadow: "#A2A13C", wellRim: "#ECE77F", wellLip: nil,
    selection: "#D3BD58",
    agents: ["#D1AFDE", "#EEA8B5", "#A1C497", "#75C9C0", "#CAB550"],
    ansi: [
      "#33321F", "#D44158", "#288664", "#977026", "#3275D7", "#915ECC",
      "#298192", "#6A684C", "#88865E", "#E4274A", "#2E8566", "#A06D14",
      "#1271F1", "#854DFF", "#298290", "#3E3D28",
    ])

  fileprivate static let sunriseGold = make(
    screen: "#EDB964", canvas: "#E4B15C", face: "#EDB964", hover: "#DEAB57",
    well: "#D8A652", ink: "#362E22", secondary: "#645948", muted: "#84755B",
    accent: "#A84500", onAccent: "#FFFFFF", shadowDark: "#B38531",
    shadowLight: "#FFCA73", wellShadow: "#B38531", wellRim: "#FFCA73", wellLip: nil,
    selection: "#DE9F4E",
    agents: ["#BE9ECC", "#DB96A4", "#90B386", "#63B7AE", "#D59746"],
    ansi: [
      "#332B1F", "#C72D45", "#247859", "#876422", "#2768C7", "#864DC7",
      "#247484", "#645948", "#84755B", "#D01A3B", "#29775B", "#906112",
      "#0D65DB", "#793CFF", "#257481", "#362E22",
    ])

  fileprivate static let deepVine = make(
    screen: "#3B3D26", canvas: "#32341E", face: "#3B3D26", hover: "#464831",
    well: "#242711", ink: "#F1F2E9", secondary: "#BFC0AE", muted: "#A1A38C",
    accent: "#F5C64C", onAccent: "#2C2511", shadowDark: "#22240F",
    shadowLight: "#4A4C35", wellShadow: "#131400", wellRim: "#3F412A",
    wellLip: "#060700", selection: "#736631",
    agents: ["#392840", "#47232C", "#1A3915", "#003531", "#6B5F2A"],
    ansi: [
      "#1D1E12", "#EF4747", "#00D9A3", "#F59E0B", "#7573FF", "#EA00E9",
      "#00A5B2", "#BFC0AE", "#A1A38C", "#EF4747", "#00D9A3", "#FFAA00",
      "#7673FF", "#EA00E9", "#00E5E5", "#F1F2E9",
    ])

  fileprivate static let deepForest = make(
    screen: "#223D22", canvas: "#1A341B", face: "#223D22", hover: "#2E482E",
    well: "#0D270E", ink: "#E9F2E9", secondary: "#AEC0AE", muted: "#8CA38C",
    accent: "#B2D353", onAccent: "#252C11", shadowDark: "#0B240B",
    shadowLight: "#324C32", wellShadow: "#001200", wellRim: "#264126",
    wellLip: "#000100", selection: "#4D6A31",
    agents: ["#392840", "#47232C", "#3A5A33", "#003531", "#46632A"],
    ansi: [
      "#101C10", "#EF4444", "#00D9A3", "#F59E0B", "#6F6DFF", "#E100E0",
      "#00A5B2", "#AEC0AE", "#8CA38C", "#EF4444", "#00D9A3", "#FFAA00",
      "#706CFF", "#E500E5", "#00E5E5", "#E9F2E9",
    ])

  fileprivate static let midnightTeal = make(
    screen: "#062635", canvas: "#00202E", face: "#062635", hover: "#133140",
    well: "#001019", ink: "#E9EFF2", secondary: "#AEBAC0", muted: "#8C9CA3",
    accent: "#7FA7FF", onAccent: "#001F45", shadowDark: "#000F18",
    shadowLight: "#1A3746", wellShadow: "#000104", wellRim: "#0B2A39",
    wellLip: "#000104", selection: "#2A4D72",
    agents: ["#402D48", "#4F2831", "#253920", "#003C37", "#22466A"],
    ansi: [
      "#010507", "#EF4444", "#00D9A3", "#F59E0B", "#5452FF", "#C100C0",
      "#00A5B2", "#AEBAC0", "#8C9CA3", "#EF4444", "#00D9A3", "#FFAA00",
      "#5651FF", "#E500E5", "#00E5E5", "#E9EFF2",
    ])

  fileprivate static let deepHarbor = make(
    screen: "#2B4851", canvas: "#223F48", face: "#2B4851", hover: "#37545D",
    well: "#13313A", ink: "#E9F0F2", secondary: "#AEBCC0", muted: "#8C9DA3",
    accent: "#19B2FF", onAccent: "#002237", shadowDark: "#112F37",
    shadowLight: "#3B5861", wellShadow: "#001E26", wellRim: "#304D56",
    wellLip: "#00151B", selection: "#266885",
    agents: ["#3B2942", "#49252D", "#21351D", "#003733", "#1C617D"],
    ansi: [
      "#192A30", "#F15D5D", "#00D9A3", "#F59E0B", "#8280FF", "#FB00FA",
      "#00A5B2", "#AEBCC0", "#8C9DA3", "#F15D5D", "#00D9A3", "#FFAA00",
      "#8380FF", "#FB00FA", "#00E5E5", "#E9F0F2",
    ])

  private static func make(
    screen: String, canvas: String, face: String, hover: String, well: String,
    ink: String, secondary: String, muted: String, accent: String, onAccent: String,
    shadowDark: String, shadowLight: String, wellShadow: String, wellRim: String,
    wellLip: String?, selection: String, agents: [String], ansi: [String],
    syntaxType: String? = nil, diffAdded: String? = nil
  ) -> SoyehtThemePalette {
    SoyehtThemePalette(
      screen: screen, canvas: canvas, face: face, hover: hover, well: well, ink: ink,
      secondary: secondary, muted: muted, accent: accent, onAccent: onAccent,
      shadowDark: shadowDark, shadowLight: shadowLight, wellShadow: wellShadow,
      wellRim: wellRim, wellLip: wellLip, selection: selection, agentPlates: agents, ansi: ansi,
      syntaxType: syntaxType ?? ansi[13], diffAdded: diffAdded ?? ansi[10])
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
