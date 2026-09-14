import SwiftUI

public struct SoyehtThemeColorSet {
  public let canvas: Color
  public let face: Color
  public let well: Color
  public let hover: Color
  public let ink: Color
  public let secondary: Color
  public let muted: Color
  public let accent: Color
  public let onAccent: Color
  public let shadowDark: Color
  public let shadowLight: Color
  public let wellShadow: Color
  public let wellRim: Color
  public let wellLip: Color?
  public let selection: Color
  public let success: Color
  public let warning: Color
  public let danger: Color
  public let syntaxType: Color
  public let diffAdded: Color
  public let agentRose: Color
  public let agentBlue: Color
  public let agentGreen: Color
  public let agentSand: Color

  public init(palette: SoyehtThemePalette) {
    canvas = Self.color(palette.canvas)
    face = Self.color(palette.face)
    well = Self.color(palette.well)
    hover = Self.color(palette.hover)
    ink = Self.color(palette.ink)
    secondary = Self.color(palette.secondary)
    muted = Self.color(palette.muted)
    accent = Self.color(palette.accent)
    onAccent = Self.color(palette.onAccent)
    shadowDark = Self.color(palette.shadowDark)
    shadowLight = Self.color(palette.shadowLight)
    wellShadow = Self.color(palette.wellShadow)
    wellRim = Self.color(palette.wellRim)
    wellLip = palette.wellLip.map(Self.color)
    selection = Self.color(palette.selection)
    success = Self.color(palette.success)
    warning = Self.color(palette.warning)
    danger = Self.color(palette.danger)
    syntaxType = Self.color(palette.syntaxType)
    diffAdded = Self.color(palette.diffAdded)
    agentBlue = Self.agentColor(palette, index: 0)
    agentGreen = Self.agentColor(palette, index: 1)
    agentRose = Self.agentColor(palette, index: 2)
    agentSand = Self.agentColor(palette, index: 3)
  }

  private static func agentColor(_ palette: SoyehtThemePalette, index: Int) -> Color {
    guard palette.agentPlates.indices.contains(index) else { return color(palette.face) }
    return color(palette.agentPlates[index])
  }

  private static func color(_ value: String) -> Color {
    guard let rgb = ThemeHexColorMath.rgb8(value) else { return Color.clear }
    return Color(
      .sRGB, red: Double(rgb.0) / 255, green: Double(rgb.1) / 255,
      blue: Double(rgb.2) / 255, opacity: 1)
  }
}

@MainActor public enum SoyehtThemeColors {
  public static var palette: SoyehtThemePalette { SoyehtThemeStore.shared.activeTheme.palette }

  private static var active: SoyehtThemeColorSet { SoyehtThemeColorSet(palette: palette) }

  public static var canvas: Color { active.canvas }
  public static var face: Color { active.face }
  public static var well: Color { active.well }
  public static var hover: Color { active.hover }
  public static var ink: Color { active.ink }
  public static var secondary: Color { active.secondary }
  public static var muted: Color { active.muted }
  public static var accent: Color { active.accent }
  public static var onAccent: Color { active.onAccent }
  public static var shadowDark: Color { active.shadowDark }
  public static var shadowLight: Color { active.shadowLight }
  public static var wellShadow: Color { active.wellShadow }
  public static var wellRim: Color { active.wellRim }
  public static var wellLip: Color? { active.wellLip }
  public static var selection: Color { active.selection }
  public static var success: Color { active.success }
  public static var warning: Color { active.warning }
  public static var danger: Color { active.danger }
  public static var syntaxType: Color { active.syntaxType }
  public static var diffAdded: Color { active.diffAdded }
  public static var agentRose: Color { active.agentRose }
  public static var agentBlue: Color { active.agentBlue }
  public static var agentGreen: Color { active.agentGreen }
  public static var agentSand: Color { active.agentSand }
}
