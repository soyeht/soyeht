import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct PaneAutomationButton: View {
  @Bindable var client: EngineClient
  let pane: Pane
  @State private var showing = false

  var body: some View {
    Button {
      showing.toggle()
    } label: {
      Image(systemName: "repeat").padding(6)
    }
    .help("Loop and lead controls")
    .popover(isPresented: $showing, arrowEdge: .bottom) {
      PaneAutomationPopover(client: client, pane: pane, dismiss: { showing = false })
    }
  }
}

private struct PaneAutomationPopover: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let dismiss: () -> Void
  @State private var role = AgentPaneRole.worker
  @State private var prompt = "Pick the next task and complete it."
  @State private var interval: UInt32 = 60
  @State private var maxRounds: UInt32 = 10
  @State private var stopWhen = LoopStopWhen.three_empty
  @State private var enabled = false

  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      Text("Pane automation").font(.headline)
      Picker("Role", selection: $role) {
        Text("Worker").tag(AgentPaneRole.worker)
        Text("Lead").tag(AgentPaneRole.chief)
      }.pickerStyle(.segmented)
      Text("Lead panes can coordinate tasks and cannot complete them.")
        .font(.caption).foregroundStyle(Palette.secondary)
      Divider()
      Toggle("Run a task loop", isOn: $enabled)
      TextField("Loop prompt", text: $prompt, axis: .vertical).lineLimit(2...4)
      Stepper("Every \(interval) seconds", value: $interval, in: 5...3600, step: 5)
      Stepper("Up to \(maxRounds) rounds", value: $maxRounds, in: 1...100)
      Picker("Stop condition", selection: $stopWhen) {
        Text("Three empty results").tag(LoopStopWhen.three_empty)
        Text("Round limit").tag(LoopStopWhen.round_limit)
        Text("Explicit off").tag(LoopStopWhen.explicit_off)
      }
      if let config = client.paneLoops[pane.paneId] {
        Text("Rounds \(config.rounds ?? 0) · empty results \(config.emptyResults ?? 0)")
          .font(.caption).foregroundStyle(Palette.secondary)
      }
      HStack {
        Button("Cancel", action: dismiss)
        Spacer()
        Button("Apply", action: apply).buttonStyle(.borderedProminent)
          .disabled(enabled && prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
      }
    }.padding(20).frame(width: 360).background(Palette.face).foregroundStyle(Palette.ink)
      .onAppear(perform: load)
  }

  private func load() {
    role = pane.agent?.role ?? .worker
    guard let config = client.paneLoops[pane.paneId] else { return }
    prompt = config.prompt
    interval = config.interval
    maxRounds = config.maxRounds
    stopWhen = config.stopWhen
    enabled = config.enabled
  }

  private func apply() {
    if role != pane.agent?.role { client.setPaneRole(pane.paneId, role: role) }
    client.setLoop(
      paneId: pane.paneId, prompt: prompt, interval: interval, maxRounds: maxRounds,
      stopWhen: stopWhen, enabled: enabled)
    dismiss()
  }
}
