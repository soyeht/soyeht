import CoreText
import SoyehtClient
import SwiftUI

enum MacTheme {
  static let canvas = SoyehtThemeColors.canvas
  static let face = SoyehtThemeColors.face
  static let well = SoyehtThemeColors.well
  static let hover = SoyehtThemeColors.hover
  static let border = SoyehtThemeColors.well
  static let ink = SoyehtThemeColors.ink
  static let secondary = SoyehtThemeColors.secondary
  static let muted = SoyehtThemeColors.muted
  static let accent = SoyehtThemeColors.accent
  static let onAccent = SoyehtThemeColors.onAccent
  static let shadowDark = SoyehtThemeColors.shadowDark
  static let shadowLight = SoyehtThemeColors.shadowLight
  static let selection = SoyehtThemeColors.selection
  static let success = SoyehtThemeColors.success
  static let warning = SoyehtThemeColors.warning
  static let danger = SoyehtThemeColors.danger
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
    let lightOffset: CGFloat
    let lightBlur: CGFloat
    let darkOffset: CGFloat
    let darkBlur: CGFloat

    init(offset: CGFloat, blur: CGFloat) {
      lightOffset = offset
      lightBlur = blur
      darkOffset = offset
      darkBlur = blur
    }

    init(lightOffset: CGFloat, lightBlur: CGFloat, darkOffset: CGFloat, darkBlur: CGFloat) {
      self.lightOffset = lightOffset
      self.lightBlur = lightBlur
      self.darkOffset = darkOffset
      self.darkBlur = darkBlur
    }

    static let chip = Elevation(offset: 3, blur: 5)
    static let toggle = Elevation(offset: 2, blur: 4)
    static let taskCard = Elevation(offset: 3, blur: 6)
    static let action = Elevation(offset: 4, blur: 8)
    static let card = Elevation(offset: 4, blur: 9)
    static let pane = Elevation(offset: 5, blur: 10)
    static let pill = Elevation(offset: 6, blur: 10)
    static let panel = Elevation(offset: 9, blur: 18)
    static let balloon = Elevation(lightOffset: 4, lightBlur: 10, darkOffset: 6, darkBlur: 14)
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
          color: MacTheme.shadowLight, radius: elevation.lightBlur / 2,
          x: -elevation.lightOffset, y: -elevation.lightOffset
        )
        .shadow(
          color: MacTheme.shadowDark, radius: elevation.darkBlur / 2,
          x: elevation.darkOffset, y: elevation.darkOffset)
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
  var elevation = MacTheme.Elevation.pill

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
        .neoRaised(radius: MacTheme.Radius.pill, elevation: elevation)
    }
  }
}
