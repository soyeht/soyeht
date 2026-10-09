import SoyehtClient
import SwiftUI

struct GeneralPreferencesPanel: View {
  @Environment(AgentAttentionNotifications.self) private var notifications

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      Text("Agent attention")
        .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
      Toggle(
        "System alerts and Dock badges when agents need input",
        isOn: Binding(
          get: { notifications.enabled },
          set: { enabled in Task { await notifications.setEnabled(enabled) } })
      )
      .tint(MacTheme.accent)
      .accessibilityIdentifier("agent-attention-notifications")
      Text(
        "Off by default. Tabs and the app bar still show requests; Dock badges depend on macOS notification settings."
      )
      .font(MacTheme.chrome(MacTheme.TypeScale.caption))
      .foregroundStyle(MacTheme.secondary)
    }
    .frame(maxWidth: .infinity, alignment: .topLeading)
    .padding(MacTheme.Space.x14)
  }
}
