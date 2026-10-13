import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct AgentHarnessSwitchPicker: View {
  @Bindable var client: EngineClient
  let pane: Pane
  @Binding var isPresented: Bool
  @State private var harness = ""
  @State private var model = ""
  @State private var effort = ""
  @State private var submitting = false

  private var currentHarness: String { pane.agent?.harness ?? "" }
  private var selectedHarness: HarnessInfo? { client.harnesses.first { $0.id == harness } }
  private var catalogue: AgentCatalogue? { client.agentCatalogues[harness] }
  private var effortOption: ConfigOption? {
    catalogue?.options.first(where: \.isEffort)
  }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
        Text(HarnessSwitchModel.title)
          .font(MacTheme.ui(MacTheme.TypeScale.body, weight: .bold))
        Text(HarnessSwitchModel.handoverExplanation)
          .font(MacTheme.ui(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.muted)
          .fixedSize(horizontal: false, vertical: true)
      }
      .padding(.horizontal, MacTheme.Space.x14)
      .padding(.top, MacTheme.Space.x14)
      AgentHarnessPicker(
        client: client, harness: $harness, model: $model, effort: $effort,
        catalogue: catalogue, effortOption: effortOption, excludingHarness: currentHarness)
      Button(action: switchHarness) {
        Text(actionLabel)
          .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .bold))
          .frame(maxWidth: .infinity)
      }
      .buttonStyle(.panePrimary)
      .disabled(selectedHarness?.available != true || submitting)
      .padding(.horizontal, MacTheme.Space.x14)
      .padding(.bottom, MacTheme.Space.x14)
      AgentAccountSection(client: client, pane: pane, isPresented: $isPresented)
    }
    .frame(width: 448)
    .background(MacTheme.face)
    .onAppear {
      client.refreshHarnesses(rescan: false)
      selectFirstHarness()
    }
    .onChange(
      of: client.harnesses.map {
        "\($0.id):\($0.available):\(HarnessPickerModel.statusLabel(for: $0))"
      }
    ) { _, _ in selectFirstHarness() }
    .onChange(of: harness) { _, value in
      model = ""
      effort = ""
      if selectedHarness?.available == true { client.refreshCatalogue(harness: value) }
    }
  }

  private var actionLabel: String { HarnessSwitchModel.actionLabel(for: selectedHarness) }

  private func selectFirstHarness() {
    guard selectedHarness?.available != true,
      let first = HarnessSwitchModel.defaultTarget(
        in: client.harnesses, current: currentHarness,
        preferredId: client.preferredHarnessId)
    else { return }
    harness = first.id
  }

  private func switchHarness() {
    guard let target = selectedHarness, target.available, !submitting else { return }
    submitting = true
    Task { @MainActor in
      let outcome = await client.requestAgentHarnessSwitch(
        pane.paneId, to: target, model: model.isEmpty ? nil : model,
        effort: effort.isEmpty ? nil : effort)
      submitting = false
      switch outcome {
      case .switched: isPresented = false
      case .failed(let message): client.problem = message
      case .unavailable: break
      }
    }
  }
}

/// The account of a signed-in agent pane, under the harness switch.
private struct AgentAccountSection: View {
  @Bindable var client: EngineClient
  let pane: Pane
  @Binding var isPresented: Bool

  var body: some View {
    if let actions = AgentAccountActions(
      agent: pane.agent, paneState: pane.state, connected: client.connected)
    {
      AgentAccountPanel(actions: actions, style: MacTheme.agentConversation) { item in
        client.run(item, paneId: pane.paneId)
        isPresented = false
      }
    }
  }
}
