import SoyehtClient
import SwiftUI

struct GeneralPreferencesPanel: View {
  let client: EngineClient
  @Environment(AgentAttentionNotifications.self) private var notifications

  var body: some View {
    ScrollView(.vertical) { content }
  }

  private var content: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      Text("Setup")
        .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
      ReadinessChecklistView(client: client)
      Text("Agent attention")
        .padding(.top, MacTheme.Space.x10)
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
