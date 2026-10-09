import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The composer's harness chip on the iPhone and iPad, after "+". Like the Mac composer
/// it names the harness the pane runs ("Claude ▾") and restarts the pane on another
/// one; the engine hands the conversation over to the new session.
struct PhoneHarnessMenu: View {
  let client: EngineClient
  let paneId: String
  @State private var target: HarnessInfo?
  @State private var requested: String?
  @State private var failure: String?

  private var current: String? { client.panes.first { $0.paneId == paneId }?.agent?.harness }
  private var switchState: AgentHarnessSwitchView? { client.agentPanes[paneId]?.harnessSwitch }

  var body: some View {
    if let current {
      Menu {
        menuContent(current: current)
      } label: {
        label(current: current)
      }
      .menuStyle(.button)
      .buttonStyle(.plain)
      .menuIndicator(.hidden)
      .disabled(!client.canSwitchAgentHarness(paneId) || requested != nil)
      .accessibilityLabel(HarnessSwitchControl.accessibilityLabel)
      .accessibilityValue(control(current: current).title)
      .accessibilityHint(HarnessSwitchControl.accessibilityHint)
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
    if let pane = client.panes.first(where: { $0.paneId == paneId }),
      let actions = AgentAccountActions(
        agent: pane.agent, paneState: pane.state, connected: client.connected)
    {
      accountSection(actions)
    }
  }

  /// The shared account items in a native menu; the Mac shows the same items in a popover.
  /// The sign-in itself finishes in the browser on the Mac.
  @ViewBuilder private func accountSection(_ actions: AgentAccountActions) -> some View {
    Section(AgentAccountActions.sectionTitle) {
      Text(AgentAccountActions.explanation)
      ForEach(actions.items) { item in
        Button(role: item.isDestructive ? .destructive : nil) {
          client.run(item, paneId: paneId)
        } label: {
          Label(item.title, systemImage: item.systemImage)
        }
        .disabled(!actions.isEnabled)
        .accessibilityIdentifier(item.accessibilityIdentifier)
      }
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

  /// The composer's harness chip: logo, name and chevron, as on the Mac.
  private func label(current: String) -> some View {
    AgentHarnessChipLabel(
      control: control(current: current), harnessId: current, style: .standard)
  }

  /// The running harness, or the one the pane is switching to.
  private func control(current: String) -> HarnessSwitchControl {
    HarnessSwitchModel.control(
      current: current, switchState: switchState, requested: requested,
      harnesses: client.harnesses, paneName: client.panes.first { $0.paneId == paneId }?.name)
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
