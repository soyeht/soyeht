import SwiftUI

public struct AgentConversationStyle: Sendable {
  public let canvas: Color
  public let face: Color
  public let well: Color
  public let ink: Color
  public let secondary: Color
  public let muted: Color
  public let accent: Color
  public let shadowDark: Color
  public let shadowLight: Color
  public let wellShadow: Color
  public let wellRim: Color
  public let wellLip: Color?
  public let success: Color
  public let warning: Color
  public let danger: Color
  public let bodyFont: Font
  public let captionFont: Font
  public let controlFont: Font
  public let headingFont: Font
  public let monoFont: Font
  public let monoSmallFont: Font
  public let canonicalLayout: Bool

  public init(
    canvas: Color, face: Color, well: Color, ink: Color, secondary: Color, muted: Color,
    accent: Color, shadowDark: Color, shadowLight: Color, wellShadow: Color, wellRim: Color,
    wellLip: Color?, success: Color, warning: Color, danger: Color, bodyFont: Font,
    captionFont: Font, controlFont: Font, headingFont: Font, monoFont: Font,
    monoSmallFont: Font, canonicalLayout: Bool
  ) {
    self.canvas = canvas
    self.face = face
    self.well = well
    self.ink = ink
    self.secondary = secondary
    self.muted = muted
    self.accent = accent
    self.shadowDark = shadowDark
    self.shadowLight = shadowLight
    self.wellShadow = wellShadow
    self.wellRim = wellRim
    self.wellLip = wellLip
    self.success = success
    self.warning = warning
    self.danger = danger
    self.bodyFont = bodyFont
    self.captionFont = captionFont
    self.controlFont = controlFont
    self.headingFont = headingFont
    self.monoFont = monoFont
    self.monoSmallFont = monoSmallFont
    self.canonicalLayout = canonicalLayout
  }

  @MainActor public static var standard: AgentConversationStyle {
    AgentConversationStyle(
      canvas: SoyehtThemeColors.canvas, face: SoyehtThemeColors.face,
      well: SoyehtThemeColors.well, ink: SoyehtThemeColors.ink,
      secondary: SoyehtThemeColors.secondary, muted: SoyehtThemeColors.muted,
      accent: SoyehtThemeColors.accent, shadowDark: SoyehtThemeColors.shadowDark,
      shadowLight: SoyehtThemeColors.shadowLight, wellShadow: SoyehtThemeColors.wellShadow,
      wellRim: SoyehtThemeColors.wellRim, wellLip: SoyehtThemeColors.wellLip,
      success: SoyehtThemeColors.success, warning: SoyehtThemeColors.warning,
      danger: SoyehtThemeColors.danger, bodyFont: .body, captionFont: .caption,
      controlFont: .caption, headingFont: .headline, monoFont: .caption.monospaced(),
      monoSmallFont: .caption2.monospaced(), canonicalLayout: false)
  }
}
