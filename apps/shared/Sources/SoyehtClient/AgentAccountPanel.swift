import SwiftUI

/// The account of a signed-in agent pane, shown under the harness chip's switch: sign in again
/// as someone else, or sign out. The engine runs the sign-in in a terminal pane and restarts the
/// agent session afterwards, so the conversation and its history stay. Flat, palette roles only.
public struct AgentAccountPanel: View {
  let actions: AgentAccountActions
  let style: AgentConversationStyle
  let run: (AgentAccountItem) -> Void

  public init(
    actions: AgentAccountActions, style: AgentConversationStyle,
    run: @escaping (AgentAccountItem) -> Void
  ) {
    self.actions = actions
    self.style = style
    self.run = run
  }

  public var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
      Rectangle().fill(style.border).frame(height: PaneInteriorMetrics.separatorWidth)
      Text(AgentAccountActions.sectionTitle).font(style.headingFont).foregroundStyle(style.ink)
      Text(AgentAccountActions.explanation)
        .font(style.captionFont).foregroundStyle(style.secondary)
        .fixedSize(horizontal: false, vertical: true)
      ForEach(Array(actions.items.enumerated()), id: \.element.id) { index, item in
        Button {
          run(item)
        } label: {
          Text(item.title).frame(maxWidth: .infinity)
        }
        .buttonStyle(PanePillButtonStyle(role(of: item, at: index), style: style))
        .accessibilityIdentifier(item.accessibilityIdentifier)
      }
    }
    .padding(PaneInteriorMetrics.cardPadding)
    .disabled(!actions.isEnabled)
  }

  /// The first way to switch is the primary action; sign out is destructive.
  private func role(of item: AgentAccountItem, at index: Int) -> PanePillButtonStyle.Role {
    if item.isDestructive { return .destructive }
    return index == 0 ? .primary : .secondary
  }
}
