import SwiftUI

/// Colors and fonts of the agent conversation. The approved pane interior is flat: the pane
/// shell is the only raised surface, so this style carries no shadow colors at all. Every
/// color is a role of the active palette (`SoyehtThemeColorSet`).
public struct AgentConversationStyle: Sendable {
  /// Fill of flat cards inside the pane: messages, permission and question cards, composer.
  public let canvas: Color
  /// The pane face behind the transcript; also the label color of the primary button.
  public let face: Color
  /// Fill of secondary buttons, code wells and the user's message bubble on a phone.
  public let well: Color
  /// Thin separators between rows.
  public let border: Color
  public let ink: Color
  public let secondary: Color
  public let muted: Color
  public let accent: Color
  public let success: Color
  public let warning: Color
  public let danger: Color
  public let bodyFont: Font
  public let captionFont: Font
  public let controlFont: Font
  public let headingFont: Font
  public let monoFont: Font
  public let monoSmallFont: Font
  /// The Mac's denser metrics, shared with the iPad when it mirrors the Mac.
  public let canonicalLayout: Bool

  /// The only public way to build a style: every color comes from the palette's roles.
  public init(
    colors: SoyehtThemeColorSet, bodyFont: Font, captionFont: Font, controlFont: Font,
    headingFont: Font, monoFont: Font, monoSmallFont: Font, canonicalLayout: Bool
  ) {
    self.init(
      canvas: colors.canvas, face: colors.face, well: colors.well, border: colors.border,
      ink: colors.ink, secondary: colors.secondary, muted: colors.muted, accent: colors.accent,
      success: colors.success, warning: colors.warning, danger: colors.danger,
      bodyFont: bodyFont, captionFont: captionFont, controlFont: controlFont,
      headingFont: headingFont, monoFont: monoFont, monoSmallFont: monoSmallFont,
      canonicalLayout: canonicalLayout)
  }

  init(
    canvas: Color, face: Color, well: Color, border: Color, ink: Color, secondary: Color,
    muted: Color, accent: Color, success: Color, warning: Color, danger: Color,
    bodyFont: Font, captionFont: Font, controlFont: Font, headingFont: Font, monoFont: Font,
    monoSmallFont: Font, canonicalLayout: Bool
  ) {
    self.canvas = canvas
    self.face = face
    self.well = well
    self.border = border
    self.ink = ink
    self.secondary = secondary
    self.muted = muted
    self.accent = accent
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

  /// The approved Mac pane interior: Inter for the interface, JetBrains Mono for code.
  public static func pane(_ colors: SoyehtThemeColorSet) -> AgentConversationStyle {
    typealias Size = PaneInteriorMetrics
    return AgentConversationStyle(
      colors: colors, bodyFont: PaneInteriorFont.ui(Size.bodyPointSize),
      captionFont: PaneInteriorFont.ui(Size.captionPointSize),
      controlFont: PaneInteriorFont.ui(Size.captionPointSize, weight: .medium),
      headingFont: PaneInteriorFont.ui(Size.headingPointSize, weight: .heavy),
      monoFont: PaneInteriorFont.mono(Size.monoPointSize),
      monoSmallFont: PaneInteriorFont.mono(Size.monoSmallPointSize), canonicalLayout: true)
  }

  /// The phone keeps the system text styles so Dynamic Type still scales the transcript.
  @MainActor public static var standard: AgentConversationStyle {
    AgentConversationStyle(
      colors: SoyehtThemeColorSet(palette: SoyehtThemeColors.palette), bodyFont: .body,
      captionFont: .caption, controlFont: .caption, headingFont: .headline,
      monoFont: .caption.monospaced(), monoSmallFont: .caption2.monospaced(),
      canonicalLayout: false)
  }
}
