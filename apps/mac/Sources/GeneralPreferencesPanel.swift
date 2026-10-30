import AppKit
import SoyehtClient
import SoyehtProtocol
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
        "On by default. Tabs and the app bar still show requests; "
          + "Dock badges depend on macOS notification settings."
      )
      .font(MacTheme.chrome(MacTheme.TypeScale.caption))
      .foregroundStyle(MacTheme.secondary)
      UpdatesSection()
    }
    .frame(maxWidth: .infinity, alignment: .topLeading)
    .padding(MacTheme.Space.x14)
  }
}

/// Whether Soyeht asks for a newer version, what it found and the buttons that open the package
/// and the release notes in the browser (ADR 0122). The desktop client shows the same sentences.
private struct UpdatesSection: View {
  @Environment(UpdateChecker.self) private var updates

  var body: some View {
    let shown = updates.presentation
    VStack(alignment: .leading, spacing: MacTheme.Space.x8) {
      Text(UpdateText.title)
        .padding(.top, MacTheme.Space.x10)
        .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
      Text(UpdateText.summary)
        .font(MacTheme.chrome(MacTheme.TypeScale.caption))
        .foregroundStyle(MacTheme.secondary)
      Toggle(
        UpdateText.toggle,
        isOn: Binding(get: { updates.enabled }, set: { updates.setEnabled($0) })
      )
      .tint(MacTheme.accent)
      .accessibilityIdentifier("update-check-toggle")
      HStack(spacing: MacTheme.Space.x10) {
        Text(shown.headline)
          .font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .accessibilityIdentifier("update-headline")
        Spacer()
        if updates.enabled {
          Button(UpdateText.checkNow) { Task { await updates.checkNow() } }
            .buttonStyle(.paneSecondary)
            .accessibilityIdentifier("update-check-now")
        }
      }
      if let warning = shown.warning {
        Text(warning)
          .font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.warning)
      }
      if let offer = updates.offer, let download = shown.download {
        HStack(spacing: MacTheme.Space.x8) {
          openButton(download, address: offer.package.url, id: "update-download")
          openButton(UpdateText.notes, address: offer.notesURL, id: "update-notes")
        }
      }
    }
  }

  private func openButton(_ title: String, address: String, id: String) -> some View {
    Button(title) {
      if let url = URL(string: address) { NSWorkspace.shared.open(url) }
    }
    .buttonStyle(.paneSecondary)
    .accessibilityIdentifier(id)
  }
}
