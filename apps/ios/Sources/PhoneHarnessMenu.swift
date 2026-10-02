import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The agent pane's harness menu, next to the permission-mode menu. It shows the
/// harness the pane runs and restarts the pane on another one; the engine hands the
/// conversation over to the new session, as it does from the Mac pane header.
struct PhoneHarnessMenu: View {
  let client: EngineClient
  let paneId: String
  @Environment(\.horizontalSizeClass) private var sizeClass
  @State private var target: HarnessInfo?
  @State private var requested: String?
  @State private var failure: String?

  private var current: String? { client.panes.first { $0.paneId == paneId }?.agent?.harness }
  private var switchState: AgentHarnessSwitchView? { client.agentPanes[paneId]?.harnessSwitch }
  private var busy: Bool { requested != nil || HarnessSwitchModel.isSwitching(switchState) }

  var body: some View {
    if let current {
      Menu {
        menuContent(current: current)
      } label: {
        label(current: current)
      }
      .menuStyle(.button)
      .buttonStyle(.bordered)
      .tint(PhonePalette.secondary)
      .disabled(!client.canSwitchAgentHarness(paneId) || requested != nil)
      .accessibilityLabel("Agent")
      .accessibilityValue(labelText(current: current))
      .accessibilityHint("Switches this pane to another agent and hands the conversation over")
      .accessibilityIdentifier("agent-harness-menu")
      .task(id: client.connected) {
        if client.connected { client.refreshHarnesses() }
      }
      .confirmationDialog(
        HarnessSwitchModel.actionLabel(for: target) + "?",
        isPresented: Binding(get: { target != nil }, set: { if !$0 { target = nil } }),
        titleVisibility: .visible, presenting: target
      ) { item in
        Button(HarnessSwitchModel.actionLabel(for: item)) { switchHarness(to: item) }
          .accessibilityIdentifier("agent-harness-confirm")
        Button("Cancel", role: .cancel) {}
      } message: { _ in
        Text(HarnessSwitchModel.handoverExplanation)
      }
      .alert(
        "Could not switch agent",
        isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })
      ) {
        Button("OK") { failure = nil }
      } message: {
        Text(failure ?? "")
      }
    }
  }

  @ViewBuilder private func menuContent(current: String) -> some View {
    let layout = HarnessSwitchModel.layout(client.harnesses, current: current)
    Section(HarnessSwitchModel.title) {
      Text(HarnessSwitchModel.handoverExplanation)
    }
    if layout.isEmpty {
      Text(client.harnessesLoading ? "Loading agents…" : "No other agents")
    }
    ForEach(layout.sections, id: \.group) { section in
      Section(section.group.title) {
        ForEach(section.items, id: \.id, content: harnessButton)
      }
    }
    if !layout.more.isEmpty {
      Menu(HarnessPickerLayout.moreTitle) {
        ForEach(layout.more, id: \.id, content: harnessButton)
      }
      .accessibilityIdentifier("agent-harness-more")
    }
    Divider()
    Button {
      client.refreshHarnesses(rescan: true)
    } label: {
      Label("Check agents again", systemImage: "arrow.clockwise")
    }
  }

  private func harnessButton(_ item: HarnessInfo) -> some View {
    Button {
      target = item
    } label: {
      Text(item.name)
      Text(HarnessPickerModel.statusLabel(for: item))
    }
    .disabled(!item.available)
    .accessibilityIdentifier("agent-harness-\(item.id)")
  }

  private func label(current: String) -> some View {
    HStack(spacing: 5) {
      if busy {
        ProgressView().controlSize(.mini)
      } else {
        Image(systemName: "arrow.triangle.2.circlepath")
      }
      Text(labelText(current: current)).lineLimit(1).truncationMode(.tail)
    }
    .frame(maxWidth: sizeClass == .regular ? nil : 120)
  }

  /// The running harness, or the one the pane is switching to.
  private func labelText(current: String) -> String {
    let shown = busy ? (switchState?.toHarness ?? requested ?? current) : current
    return HarnessSwitchModel.displayName(of: shown, in: client.harnesses)
  }

  private func switchHarness(to item: HarnessInfo) {
    guard requested == nil else { return }
    requested = item.id
    target = nil
    Task {
      let outcome = await client.requestAgentHarnessSwitch(paneId, to: item)
      requested = nil
      if case .failed(let message) = outcome { failure = message }
    }
  }
}
