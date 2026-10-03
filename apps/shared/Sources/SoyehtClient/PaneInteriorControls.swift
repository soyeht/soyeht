import SwiftUI

/// Metrics of the approved flat pane interior ("Apple limpo", design/soyeht-desktop.pen,
/// frames "Interior da pane · APROVADO"). The pane shell is the only raised surface; cards,
/// rows and controls inside it are flat.
public enum PaneInteriorMetrics {
  public static let cardRadius: CGFloat = 12
  public static let composerRadius: CGFloat = 14
  public static let wellRadius: CGFloat = 8
  public static let cardPadding: CGFloat = 14
  /// The phone's wider margin around the transcript and its cards.
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
  /// Type sizes of the approved interior: Inter for prose and controls, mono for code.
  public static let bodyPointSize: CGFloat = 14
  public static let captionPointSize: CGFloat = 12
  public static let headingPointSize: CGFloat = 13
  public static let monoPointSize: CGFloat = 12
  public static let monoSmallPointSize: CGFloat = 11
  public static let pressedOpacity = 0.78
  public static let disabledOpacity = 0.45
}

/// A flat pill button: `primary` is `ink` with `face` text, `secondary` is `well` with `ink`
/// text, and `destructive` is `well` with `danger` text. No shadow in any state.
public struct PanePillButtonStyle: ButtonStyle {
  public enum Role: Sendable { case primary, secondary, destructive }

  let role: Role
  let font: Font
  let ink: Color
  let face: Color
  let well: Color
  let danger: Color

  public init(
    _ role: Role, font: Font, ink: Color, face: Color, well: Color, danger: Color
  ) {
    self.role = role
    self.font = font
    self.ink = ink
    self.face = face
    self.well = well
    self.danger = danger
  }

  public init(_ role: Role, style: AgentConversationStyle) {
    self.init(
      role, font: style.controlFont.weight(.bold), ink: style.ink, face: style.face,
      well: style.well, danger: style.danger)
  }

  public func makeBody(configuration: Configuration) -> some View {
    PanePill(
      label: configuration.label, pressed: configuration.isPressed, font: font,
      foreground: foreground, fill: role == .primary ? ink : well)
  }

  private var foreground: Color {
    switch role {
    case .primary: face
    case .secondary: ink
    case .destructive: danger
    }
  }
}

private struct PanePill<Label: View>: View {
  @Environment(\.isEnabled) private var isEnabled
  let label: Label
  let pressed: Bool
  let font: Font
  let foreground: Color
  let fill: Color

  var body: some View {
    label
      .font(font)
      .foregroundStyle(foreground)
      .lineLimit(1)
      .padding(.horizontal, PaneInteriorMetrics.pillHorizontalPadding)
      .padding(.vertical, PaneInteriorMetrics.pillVerticalPadding)
      .background(fill, in: Capsule())
      .contentShape(Capsule())
      .opacity(
        !isEnabled
          ? PaneInteriorMetrics.disabledOpacity
          : pressed ? PaneInteriorMetrics.pressedOpacity : 1)
  }
}

/// The circular send button of the approved composer: `ink` fill, `face` arrow, no shadow.
public struct PaneSendButtonLabel: View {
  let ink: Color
  let face: Color
  let font: Font

  public init(ink: Color, face: Color, font: Font) {
    self.ink = ink
    self.face = face
    self.font = font
  }

  public var body: some View {
    Image(systemName: "arrow.up")
      .font(font)
      .foregroundStyle(face)
      .frame(
        width: PaneInteriorMetrics.sendButtonSize, height: PaneInteriorMetrics.sendButtonSize
      )
      .background(ink, in: Circle())
  }
}

extension View {
  /// A flat card inside a pane: a filled rounded rectangle, no border and no shadow.
  public func paneInteriorFill(
    _ color: Color, radius: CGFloat = PaneInteriorMetrics.cardRadius
  ) -> some View {
    background(color, in: RoundedRectangle(cornerRadius: radius, style: .continuous))
  }

  /// A thin separator under a row, as between the approved tool rows.
  public func paneInteriorSeparator(_ color: Color) -> some View {
    overlay(alignment: .bottom) {
      Rectangle().fill(color).frame(height: PaneInteriorMetrics.separatorWidth)
    }
  }
}
