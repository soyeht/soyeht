import SoyehtProtocol
import SwiftUI

extension StateDot {
  /// The palette color of the dot's role. Which role each pane state takes is the contract's
  /// `x-clientPolicy.paneStates`; this is the one place a role becomes a color.
  @MainActor public var color: Color {
    switch self {
    case .success: SoyehtThemeColors.success
    case .accent: SoyehtThemeColors.accent
    case .warning: SoyehtThemeColors.warning
    case .muted: SoyehtThemeColors.muted
    case .secondary: SoyehtThemeColors.secondary
    }
  }
}
