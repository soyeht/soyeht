import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct AppRail: View {
  @Bindable var client: EngineClient
  let destination: WorkspaceDestination
  let onSelect: (WorkspaceDestination) -> Void
  let onSettings: () -> Void
  let onUsageDetails: (UsageProvider) -> Void

  var body: some View {
    VStack(spacing: MacTheme.Space.x6) {
      ForEach(items, id: \.destination) { item in
        AppRailButton(
          icon: item.icon,
          label: item.label,
          selected: destination == item.destination,
          action: { onSelect(item.destination) })
      }
      Rectangle()
        .fill(MacTheme.shadowDark)
        .frame(width: 24, height: 1)
        .padding(.vertical, MacTheme.Space.x2)
      ForEach(client.usageReadings, id: \.provider) { reading in
        UsageRing(client: client, reading: reading, onDetails: onUsageDetails)
      }
      Spacer(minLength: 0)
      AppRailButton(icon: "gearshape", label: "Settings", selected: false, action: onSettings)
    }
    .padding(.vertical, MacTheme.Space.x12)
    .padding(.horizontal, MacTheme.Space.x8)
    .frame(width: 56)
    .frame(maxHeight: .infinity)
    .background(MacTheme.canvas)
    .accessibilityIdentifier("app-bar")
    .task { await refreshWhileOpen() }
  }

  private func refreshWhileOpen() async {
    try? await client.refreshUsage()
    while !Task.isCancelled {
      do { try await Task.sleep(for: .seconds(300)) } catch { return }
      try? await client.refreshUsage()
    }
  }

  private var items: [AppRailItem] {
    [
      AppRailItem(destination: .agents, icon: "cpu", label: "Agents"),
      AppRailItem(destination: .editor, icon: "doc.text", label: "Editor"),
      AppRailItem(destination: .web, icon: "globe", label: "Web"),
      AppRailItem(destination: .tasks, icon: "checklist", label: "Tasks"),
      AppRailItem(destination: .terminal, icon: "terminal", label: "Terminal"),
      AppRailItem(destination: .phone, icon: "iphone", label: "iPhone"),
      AppRailItem(destination: .texts, icon: "feather", label: "Texts"),
      AppRailItem(
        destination: .presentations, icon: "rectangle.on.rectangle", label: "Presentations"),
      AppRailItem(destination: .screenplays, icon: "movieclapper", label: "Screenplays"),
    ]
  }
}

private struct AppRailItem {
  let destination: WorkspaceDestination
  let icon: String
  let label: String
}

private struct AppRailButton: View {
  let icon: String
  let label: String
  let selected: Bool
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      Image(systemName: icon)
        .font(.system(size: 18, weight: .medium))
        .foregroundStyle(selected ? MacTheme.accent : MacTheme.secondary)
        .frame(width: 36, height: 36)
        .background {
          if selected {
            RoundedRectangle(cornerRadius: MacTheme.Radius.compact, style: .continuous)
              .fill(MacTheme.well)
          }
        }
    }
    .help(label)
    .accessibilityLabel(label)
    .accessibilityIdentifier("app-bar-\(label.lowercased())")
  }
}
