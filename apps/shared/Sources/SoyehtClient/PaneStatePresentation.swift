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

/// The state dot at the head of a pane's header and tab.
@MainActor public struct PaneStateDot: View {
  private let state: PaneState

  public init(_ state: PaneState) {
    self.state = state
  }

  public var body: some View {
    Circle().fill(state.dot.color)
      .frame(
        width: PaneLayoutContentMetrics.genericPaneHeaderStateDotWidth,
        height: PaneLayoutContentMetrics.genericPaneHeaderStateDotWidth)
  }
}
