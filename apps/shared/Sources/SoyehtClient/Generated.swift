// Generated from contracts/design-tokens.json. Do not edit.
import CoreGraphics

/// The reviewed themes, in the order of `palettes` in the token file. The first one is
/// the seeded default.
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

extension SoyehtThemePalette {
  static let milk = make(
    screen: "#E8EDF4", canvas: "#E0E5EC", face: "#E8EDF4", hover: "#DCE2EA", well: "#D8DEE8",
    ink: "#3E4A66", secondary: "#6E7A96", muted: "#8E9AB3", accent: "#5B7CFA", onAccent: "#FFFFFF",
    shadowDark: "#A6B4C8", shadowLight: "#FFFFFF", wellShadow: "#A6B4C8", wellRim: "#FFFFFF",
    wellLip: nil, selection: "#C9D6EE",
    presentationLetterbox: "#151A24",
    agents: ["#D8E0FE", "#CDE7DD", "#F6D6DB", "#EEE3CD"],
    ansi: [
      "#1F2633", "#D9556A", "#2E9A73", "#B98A2E", "#3D7DD9", "#9A6BD0", "#2E93A6", "#8E9AB3",
      "#6E7A96", "#ED6F86", "#3FB68B", "#E9B04E", "#5B9DF5", "#B08CFF", "#3FB6C8", "#263043",
    ], syntaxType: "#8E5BFA", diffAdded: "#3F9C6A")

  static let paleMist = make(
    screen: "#C0C8CA", canvas: "#B8C0C2", face: "#C0C8CA", hover: "#B2BABC", well: "#AFB7B9",
    ink: "#25363B", secondary: "#4B6268", muted: "#5E8089", accent: "#0F64D1", onAccent: "#FFFFFF",
    shadowDark: "#8C9395", shadowLight: "#D1D9DB", wellShadow: "#8C9395", wellRim: "#D1D9DB",
    wellLip: nil, selection: "#99B2CC",
    presentationLetterbox: "#172226",
    agents: ["#C3A2D1", "#E19BA9", "#94B88A", "#68BCB3", "#91AAC4"],
    ansi: [
      "#1F2F33", "#CE2E48", "#257C5C", "#8B6823", "#286CCD", "#8952C8", "#257887", "#4B6268",
      "#5E8089", "#D71B3D", "#2B7B5E", "#946412", "#0E68E2", "#7E42FF", "#267885", "#25363B",
    ])

  static let mistyBlue = make(
    screen: "#83A7B4", canvas: "#7B9FAC", face: "#83A7B4", hover: "#769AA7", well: "#7296A2",
    ink: "#0C1012", secondary: "#37464C", muted: "#4A626B", accent: "#004497", onAccent: "#FFFFFF",
    shadowDark: "#517480", shadowLight: "#93B7C5", wellShadow: "#517480", wellRim: "#93B7C5",
    wellLip: nil, selection: "#6691AE",
    presentationLetterbox: "#0B151A",
    agents: ["#9E7FAB", "#B97785", "#719368", "#41978F", "#5E89A6"],
    ansi: [
      "#1F2E33", "#9C2336", "#1C5D46", "#694E1A", "#1E519B", "#6B35A8", "#1C5A66", "#37464C",
      "#4A626B", "#A3142F", "#205C47", "#6F4C0E", "#0A4FAA", "#4E00FB", "#1D5A64", "#0C1012",
    ])

  static let sunlitChartreuse = make(
    screen: "#DCD870", canvas: "#D2CE67", face: "#DCD870", hover: "#CDCA63", well: "#C6C35C",
    ink: "#3E3D28", secondary: "#6A684C", muted: "#88865E", accent: "#B55C03", onAccent: "#FFFFFF",
    shadowDark: "#A2A13C", shadowLight: "#ECE77F", wellShadow: "#A2A13C", wellRim: "#ECE77F",
    wellLip: nil, selection: "#D3BD58",
    presentationLetterbox: "#242315",
    agents: ["#D1AFDE", "#EEA8B5", "#A1C497", "#75C9C0", "#CAB550"],
    ansi: [
      "#33321F", "#D44158", "#288664", "#977026", "#3275D7", "#915ECC", "#298192", "#6A684C",
      "#88865E", "#E4274A", "#2E8566", "#A06D14", "#1271F1", "#854DFF", "#298290", "#3E3D28",
    ])

  static let sunriseGold = make(
    screen: "#EDB964", canvas: "#E4B15C", face: "#EDB964", hover: "#DEAB57", well: "#D8A652",
    ink: "#362E22", secondary: "#645948", muted: "#84755B", accent: "#A84500", onAccent: "#FFFFFF",
    shadowDark: "#B38531", shadowLight: "#FFCA73", wellShadow: "#B38531", wellRim: "#FFCA73",
    wellLip: nil, selection: "#DE9F4E",
    presentationLetterbox: "#241D14",
    agents: ["#BE9ECC", "#DB96A4", "#90B386", "#63B7AE", "#D59746"],
    ansi: [
      "#332B1F", "#C72D45", "#247859", "#876422", "#2768C7", "#864DC7", "#247484", "#645948",
      "#84755B", "#D01A3B", "#29775B", "#906112", "#0D65DB", "#793CFF", "#257481", "#362E22",
    ])

  static let deepVine = make(
    screen: "#3B3D26", canvas: "#32341E", face: "#3B3D26", hover: "#464831", well: "#242711",
    ink: "#F1F2E9", secondary: "#BFC0AE", muted: "#A1A38C", accent: "#F5C64C", onAccent: "#2C2511",
    shadowDark: "#22240F", shadowLight: "#4A4C35", wellShadow: "#131400", wellRim: "#3F412A",
    wellLip: "#060700", selection: "#736631",
    presentationLetterbox: "#11120B",
    agents: ["#392840", "#47232C", "#1A3915", "#003531", "#6B5F2A"],
    ansi: [
      "#1D1E12", "#EF4747", "#00D9A3", "#F59E0B", "#7573FF", "#EA00E9", "#00A5B2", "#BFC0AE",
      "#A1A38C", "#EF4747", "#00D9A3", "#FFAA00", "#7673FF", "#EA00E9", "#00E5E5", "#F1F2E9",
    ])

  static let deepForest = make(
    screen: "#223D22", canvas: "#1A341B", face: "#223D22", hover: "#2E482E", well: "#0D270E",
    ink: "#E9F2E9", secondary: "#AEC0AE", muted: "#8CA38C", accent: "#B2D353", onAccent: "#252C11",
    shadowDark: "#0B240B", shadowLight: "#324C32", wellShadow: "#001200", wellRim: "#264126",
    wellLip: "#000100", selection: "#4D6A31",
    presentationLetterbox: "#071008",
    agents: ["#392840", "#47232C", "#3A5A33", "#003531", "#46632A"],
    ansi: [
      "#101C10", "#EF4444", "#00D9A3", "#F59E0B", "#6F6DFF", "#E100E0", "#00A5B2", "#AEC0AE",
      "#8CA38C", "#EF4444", "#00D9A3", "#FFAA00", "#706CFF", "#E500E5", "#00E5E5", "#E9F2E9",
    ])

  static let midnightTeal = make(
    screen: "#062635", canvas: "#00202E", face: "#062635", hover: "#133140", well: "#001019",
    ink: "#E9EFF2", secondary: "#AEBAC0", muted: "#8C9CA3", accent: "#7FA7FF", onAccent: "#001F45",
    shadowDark: "#000F18", shadowLight: "#1A3746", wellShadow: "#000104", wellRim: "#0B2A39",
    wellLip: "#000104", selection: "#2A4D72",
    presentationLetterbox: "#000B10",
    agents: ["#402D48", "#4F2831", "#253920", "#003C37", "#22466A"],
    ansi: [
      "#010507", "#EF4444", "#00D9A3", "#F59E0B", "#5452FF", "#C100C0", "#00A5B2", "#AEBAC0",
      "#8C9CA3", "#EF4444", "#00D9A3", "#FFAA00", "#5651FF", "#E500E5", "#00E5E5", "#E9EFF2",
    ])

  static let deepHarbor = make(
    screen: "#2B4851", canvas: "#223F48", face: "#2B4851", hover: "#37545D", well: "#13313A",
    ink: "#E9F0F2", secondary: "#AEBCC0", muted: "#8C9DA3", accent: "#19B2FF", onAccent: "#002237",
    shadowDark: "#112F37", shadowLight: "#3B5861", wellShadow: "#001E26", wellRim: "#304D56",
    wellLip: "#00151B", selection: "#266885",
    presentationLetterbox: "#0B161A",
    agents: ["#3B2942", "#49252D", "#21351D", "#003733", "#1C617D"],
    ansi: [
      "#192A30", "#F15D5D", "#00D9A3", "#F59E0B", "#8280FF", "#FB00FA", "#00A5B2", "#AEBCC0",
      "#8C9DA3", "#F15D5D", "#00D9A3", "#FFAA00", "#8380FF", "#FB00FA", "#00E5E5", "#E9F0F2",
    ])
}

/// The design tokens of `contracts/design-tokens.json`: the one source of the Mac and the
/// desktop clients' radii, spacing, type scale, shadows and layout metrics.
public enum DesignTokens {
  public enum Radius {
    public static let compact: CGFloat = 10
    public static let control: CGFloat = 12
    public static let card: CGFloat = 20
    public static let pill: CGFloat = 999
  }

  public enum Space {
    public static let x2: CGFloat = 2
    public static let x3: CGFloat = 3
    public static let x4: CGFloat = 4
    public static let x5: CGFloat = 5
    public static let x6: CGFloat = 6
    public static let x7: CGFloat = 7
    public static let x8: CGFloat = 8
    public static let x10: CGFloat = 10
    public static let x12: CGFloat = 12
    public static let x14: CGFloat = 14
    public static let x16: CGFloat = 16
    public static let x18: CGFloat = 18
  }

  public enum TypeScale {
    public static let micro: CGFloat = 10
    public static let small: CGFloat = 11
    public static let control: CGFloat = 12
    public static let caption: CGFloat = 13
    public static let body: CGFloat = 15
    public static let subheading: CGFloat = 18
    public static let heading: CGFloat = 20
    public static let title: CGFloat = 28
  }

  public enum PaneShell {
    public static let cornerRadius: CGFloat = 20
    public static let shadowOffset: CGFloat = 5
    public static let shadowBlur: CGFloat = 10
  }

  public enum PaneLayout {
    public static let paneGap: CGFloat = 18
    public static let screenPadding: CGFloat = 18
    public static let gridPadding: CGFloat = 6
    public static let paneCardInset: CGFloat = 10
    public static let paneStackSpacing: CGFloat = 8
    public static let headerHeight: CGFloat = 26
    public static let headerStateDot: CGFloat = 8
    public static let headerActionWidth: CGFloat = 24
    public static let headerTickerMinimumWidth: CGFloat = 80
    public static let terminalFontSize: CGFloat = 13
    public static let terminalFontSizeMinimum: CGFloat = 8
    public static let terminalFontSizeMaximum: CGFloat = 32
    public static let terminalFontSizeStep: CGFloat = 1
  }

  public enum Layout {
    public static let railWidth: CGFloat = 56
    public static let railButtonSize: CGFloat = 36
    public static let railDividerWidth: CGFloat = 24
    public static let topBarHeight: CGFloat = 42
    public static let topBarLeadingInset: CGFloat = 52
    public static let topBarControlHeight: CGFloat = 34
    public static let workspaceTabHeight: CGFloat = 36
    public static let workspaceTabMaximumWidth: CGFloat = 216
    public static let workspaceOverflowButtonWidth: CGFloat = 44
    public static let workspaceOverflowPopoverWidth: CGFloat = 340
    public static let workspaceOverflowPopoverMaximumHeight: CGFloat = 420
  }

  public enum Glyph {
    public static let rail: CGFloat = 18
    public static let topBarToggle: CGFloat = 15
    public static let topBarNewPane: CGFloat = 14
    public static let tabClose: CGFloat = 9
    public static let chevron: CGFloat = 11
    public static let taskToolbarIcon: CGFloat = 13
    public static let taskToolbarChevron: CGFloat = 10
  }

  public enum UsageRing {
    public static let diameter: CGFloat = 40
    public static let faceDiameter: CGFloat = 34
    public static let arcWidth: CGFloat = 3
    public static let trackWidth: CGFloat = 5
    public static let glyphSize: CGFloat = 16
    public static let percentageSize: CGFloat = 9
    public static let percentageHeight: CGFloat = 15
    public static let height: CGFloat = 55
  }

  public enum PaneInterior {
    public static let cardRadius: CGFloat = 12
    public static let composerRadius: CGFloat = 14
    public static let wellRadius: CGFloat = 8
    public static let cardPadding: CGFloat = 14
    public static let phoneInset: CGFloat = 16
    public static let cardVerticalPadding: CGFloat = 12
    public static let stackSpacing: CGFloat = 12
    public static let compactSpacing: CGFloat = 8
    public static let tightSpacing: CGFloat = 4
    public static let rowSpacing: CGFloat = 10
    public static let rowVerticalPadding: CGFloat = 8
    public static let separatorWidth: CGFloat = 1
    public static let sendButtonSize: CGFloat = 28
    public static let pillHorizontalPadding: CGFloat = 16
    public static let pillVerticalPadding: CGFloat = 8
    public static let bodyPointSize: CGFloat = 14
    public static let captionPointSize: CGFloat = 12
    public static let headingPointSize: CGFloat = 13
    public static let monoPointSize: CGFloat = 12
    public static let monoSmallPointSize: CGFloat = 11
    public static let pressedOpacity: CGFloat = 0.78
    public static let disabledOpacity: CGFloat = 0.45
  }

  public enum Composer {
    public static let rowSpacing: CGFloat = 10
    public static let controlSpacing: CGFloat = 4
    public static let controlHeight: CGFloat = 28
    public static let chipHorizontalPadding: CGFloat = 8
    public static let chipSpacing: CGFloat = 5
    public static let iconSize: CGFloat = 16
    public static let chevronSize: CGFloat = 11
    public static let shieldSize: CGFloat = 13
    public static let harnessLogoSize: CGFloat = 14
    public static let sendWidth: CGFloat = 40
    public static let stopSize: CGFloat = 10
    public static let stopRadius: CGFloat = 2
    public static let ringDiameter: CGFloat = 16
    public static let ringLineWidth: CGFloat = 3
    public static let contextSpacing: CGFloat = 6
    public static let contextHorizontalPadding: CGFloat = 4
    public static let chipsMinimumWidth: CGFloat = 112
    public static let contextPercentageWidth: CGFloat = 30
  }

  public enum Editor {
    public static let fontPointSize: CGFloat = 11.5
    public static let lineHeight: CGFloat = 19
    public static let gutterWidth: CGFloat = 44
    public static let textInset: CGFloat = 10
    public static let headerHeight: CGFloat = 27
    public static let tabHeight: CGFloat = 26
    public static let viewportVerticalInset: CGFloat = 20
    public static let statusHeight: CGFloat = 24
    public static let findFieldHeight: CGFloat = 24
    public static let projectPanelWidth: CGFloat = 212
  }

  public enum Fonts {
    public static let chrome = "Nunito"
    public static let ui = "Inter"
    public static let mono = "JetBrains Mono"
    public static let screenplay = "Courier Prime"
  }

  /// An outer shadow pair of a raised surface: light up and left, dark down and right.
  public struct Elevation: Sendable {
    public let lightOffset: CGFloat
    public let lightBlur: CGFloat
    public let darkOffset: CGFloat
    public let darkBlur: CGFloat

    public static let chip = Elevation(lightOffset: 3, lightBlur: 5, darkOffset: 3, darkBlur: 5)
    public static let toggle = Elevation(lightOffset: 2, lightBlur: 4, darkOffset: 2, darkBlur: 4)
    public static let taskCard = Elevation(lightOffset: 3, lightBlur: 6, darkOffset: 3, darkBlur: 6)
    public static let action = Elevation(lightOffset: 4, lightBlur: 8, darkOffset: 4, darkBlur: 8)
    public static let card = Elevation(lightOffset: 4, lightBlur: 9, darkOffset: 4, darkBlur: 9)
    public static let pill = Elevation(lightOffset: 6, lightBlur: 10, darkOffset: 6, darkBlur: 10)
    public static let panel = Elevation(lightOffset: 9, lightBlur: 18, darkOffset: 9, darkBlur: 18)
    public static let balloon = Elevation(
      lightOffset: 4, lightBlur: 10, darkOffset: 6, darkBlur: 14)
  }
}
