import CoreText
import SoyehtClient
import SwiftUI

enum MacTheme {
  static let canvas = Color(red: 224 / 255, green: 229 / 255, blue: 236 / 255)
  static let face = Color(red: 232 / 255, green: 237 / 255, blue: 244 / 255)
  static let well = Color(red: 216 / 255, green: 222 / 255, blue: 232 / 255)
  static let hover = Color(red: 220 / 255, green: 226 / 255, blue: 234 / 255)
  static let border = Color(red: 216 / 255, green: 222 / 255, blue: 232 / 255)
  static let ink = Color(red: 62 / 255, green: 74 / 255, blue: 102 / 255)
  static let secondary = Color(red: 110 / 255, green: 122 / 255, blue: 150 / 255)
  static let muted = Color(red: 142 / 255, green: 154 / 255, blue: 179 / 255)
  static let accent = Color(red: 91 / 255, green: 124 / 255, blue: 250 / 255)
  static let onAccent = Color.white
  static let shadowDark = Color(red: 166 / 255, green: 180 / 255, blue: 200 / 255)
  static let shadowLight = Color.white
  static let selection = Color(red: 201 / 255, green: 214 / 255, blue: 238 / 255)
  static let success = Color(red: 46 / 255, green: 154 / 255, blue: 115 / 255)
  static let warning = Color(red: 185 / 255, green: 138 / 255, blue: 46 / 255)
  static let danger = Color(red: 217 / 255, green: 85 / 255, blue: 106 / 255)
  static let urgent = danger
  static let agentRose = Color(red: 246 / 255, green: 214 / 255, blue: 219 / 255)
  static let agentBlue = Color(red: 216 / 255, green: 224 / 255, blue: 254 / 255)
  static let agentGreen = Color(red: 205 / 255, green: 231 / 255, blue: 221 / 255)
  static let agentSand = Color(red: 238 / 255, green: 227 / 255, blue: 205 / 255)

  enum Radius {
    static let compact: CGFloat = 10
    static let control: CGFloat = 12
    static let card: CGFloat = 20
    static let pill: CGFloat = 999
  }

  enum Space {
    static let x2: CGFloat = 2
    static let x3: CGFloat = 3
    static let x4: CGFloat = 4
    static let x5: CGFloat = 5
    static let x6: CGFloat = 6
    static let x7: CGFloat = 7
    static let x8: CGFloat = 8
    static let x10: CGFloat = 10
    static let x12: CGFloat = 12
    static let x14: CGFloat = 14
    static let x16: CGFloat = 16
    static let x18: CGFloat = 18
  }

  enum TypeScale {
    static let micro: CGFloat = 10
    static let small: CGFloat = 11
    static let control: CGFloat = 12
    static let caption: CGFloat = 13
    static let body: CGFloat = 15
    static let subheading: CGFloat = 18
    static let heading: CGFloat = 20
    static let title: CGFloat = 28
  }

  struct Elevation: Sendable {
    let offset: CGFloat
    let blur: CGFloat

    static let chip = Elevation(offset: 3, blur: 5)
    static let taskCard = Elevation(offset: 3, blur: 6)
    static let card = Elevation(offset: 4, blur: 9)
    static let pane = Elevation(offset: 5, blur: 10)
    static let pill = Elevation(offset: 6, blur: 10)
    static let panel = Elevation(offset: 9, blur: 18)

    var swiftUIRadius: CGFloat { blur / 2 }
  }

  static func chrome(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
    .custom("Nunito", size: size).weight(weight)
  }

  static func mono(_ size: CGFloat, weight: Font.Weight = .regular) -> Font {
    .custom("JetBrains Mono", size: size).weight(weight)
  }

  static func registerFonts() {
    let fonts = Bundle.main.urls(forResourcesWithExtension: "ttf", subdirectory: "Fonts") ?? []
    for font in fonts {
      _ = CTFontManagerRegisterFontsForURL(font as CFURL, .process, nil)
    }
  }

  static let agentConversation = AgentConversationStyle(
    canvas: face, face: face, well: well, ink: ink, secondary: secondary, muted: muted,
    accent: accent, shadowDark: shadowDark, shadowLight: shadowLight, success: success,
    warning: warning, danger: danger, bodyFont: chrome(TypeScale.body),
    captionFont: chrome(TypeScale.caption), controlFont: chrome(TypeScale.control),
    headingFont: chrome(TypeScale.caption, weight: .bold), monoFont: mono(TypeScale.control),
    monoSmallFont: mono(TypeScale.micro), canonicalLayout: true)
}

private struct NeoRaised: ViewModifier {
  let radius: CGFloat
  let elevation: MacTheme.Elevation

  func body(content: Content) -> some View {
    content.background {
      RoundedRectangle(cornerRadius: radius, style: .continuous)
        .fill(MacTheme.face)
        .compositingGroup()
        .shadow(
          color: MacTheme.shadowLight, radius: elevation.swiftUIRadius,
          x: -elevation.offset, y: -elevation.offset
        )
        .shadow(
          color: MacTheme.shadowDark, radius: elevation.swiftUIRadius,
          x: elevation.offset, y: elevation.offset)
    }
  }
}

private struct NeoWell: ViewModifier {
  let radius: CGFloat
  let depth: CGFloat

  func body(content: Content) -> some View {
    content.background {
      RoundedRectangle(cornerRadius: radius, style: .continuous)
        .fill(
          MacTheme.well
            .shadow(.inner(color: MacTheme.shadowDark, radius: depth, x: depth, y: depth))
            .shadow(.inner(color: MacTheme.shadowLight, radius: depth, x: -depth, y: -depth))
        )
    }
  }
}

private struct NeoAccentGlow: ViewModifier {
  let radius: CGFloat

  func body(content: Content) -> some View {
    content.background {
      RoundedRectangle(cornerRadius: radius, style: .continuous)
        .fill(MacTheme.accent)
        .shadow(color: MacTheme.accent.opacity(0.4), radius: 5, x: 4, y: 4)
        .shadow(color: MacTheme.shadowLight, radius: 3, x: -3, y: -3)
    }
  }
}

extension View {
  func neoRaised(
    radius: CGFloat = MacTheme.Radius.card,
    elevation: MacTheme.Elevation = .card
  ) -> some View {
    modifier(NeoRaised(radius: radius, elevation: elevation))
  }

  func neoWell(radius: CGFloat = MacTheme.Radius.control, depth: CGFloat = 3) -> some View {
    modifier(NeoWell(radius: radius, depth: depth))
  }

  func neoAccent(radius: CGFloat = MacTheme.Radius.control) -> some View {
    modifier(NeoAccentGlow(radius: radius))
  }

  func pickerHeading() -> some View {
    font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
      .tracking(1).foregroundStyle(MacTheme.muted)
  }
}

struct NeoPillButtonStyle: ButtonStyle {
  var accent = false

  @ViewBuilder
  func makeBody(configuration: Configuration) -> some View {
    if accent {
      configuration.label
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
        .foregroundStyle(MacTheme.onAccent)
        .padding(.horizontal, MacTheme.Space.x14)
        .padding(.vertical, MacTheme.Space.x8)
        .opacity(configuration.isPressed ? 0.78 : 1)
        .neoAccent(radius: MacTheme.Radius.pill)
    } else {
      configuration.label
        .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
        .foregroundStyle(MacTheme.ink)
        .padding(.horizontal, MacTheme.Space.x14)
        .padding(.vertical, MacTheme.Space.x8)
        .opacity(configuration.isPressed ? 0.78 : 1)
        .neoRaised(radius: MacTheme.Radius.pill, elevation: .pill)
    }
  }
}
