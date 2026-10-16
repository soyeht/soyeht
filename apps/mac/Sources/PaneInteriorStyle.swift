import SoyehtClient
import SwiftUI

/// The approved pane interior ("Apple limpo", design/soyeht-desktop.pen, frames
/// "Interior da pane · APROVADO"). The pane shell is the only neumorphic surface; everything
/// inside it is flat and colored only by palette roles. `scripts/check_pane_interior.py`
/// rejects neumorphic modifiers, shadows and literal colors in the pane interior sources.
extension View {
  /// The raised pane card in the active palette (`PaneShell`). The one neumorphic surface a
  /// pane draws; its halo needs `PaneShellMetrics.halo` of unclipped room on every side. Every
  /// scroll view inside the pane gets thin overlay scrollers that hide at rest.
  func paneShell() -> some View {
    modifier(PaneShell(colors: SoyehtThemeColorSet(palette: SoyehtThemeColors.palette)))
      .overlayScrollersWithin()
  }

  /// A flat `canvas` card inside a pane: no border, no shadow.
  func paneCard(radius: CGFloat = PaneInteriorMetrics.cardRadius) -> some View {
    paneInteriorFill(MacTheme.canvas, radius: radius)
  }

  /// A flat `well` field, code box or selected row inside a pane.
  func paneWell(radius: CGFloat = PaneInteriorMetrics.wellRadius) -> some View {
    paneInteriorFill(MacTheme.well, radius: radius)
  }
}

extension ButtonStyle where Self == PanePillButtonStyle {
  /// `ink` pill with `face` text: Allow, Save, Replace All.
  @MainActor static var panePrimary: PanePillButtonStyle { pane(.primary) }
  /// `well` pill with `ink` text.
  @MainActor static var paneSecondary: PanePillButtonStyle { pane(.secondary) }

  @MainActor private static func pane(_ role: PanePillButtonStyle.Role) -> PanePillButtonStyle {
    PanePillButtonStyle(
      role, font: MacTheme.ui(MacTheme.TypeScale.control, weight: .bold), ink: MacTheme.ink,
      face: MacTheme.face, well: MacTheme.well, danger: MacTheme.danger)
  }
}

extension PaneSendButtonLabel {
  /// The composer's send button in the active palette.
  @MainActor static var mac: PaneSendButtonLabel {
    PaneSendButtonLabel(
      ink: MacTheme.ink, face: MacTheme.face,
      font: MacTheme.ui(MacTheme.TypeScale.control, weight: .bold))
  }
}
