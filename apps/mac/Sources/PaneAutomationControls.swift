import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct PaneAutomationButton: View {
  @Binding var showing: Bool

  var body: some View {
    Button {
      showing.toggle()
    } label: {
      Image(systemName: "repeat").padding(6)
    }
    .help("Loop and lead controls")
  }
}

struct PaneAutomationPanel: View {
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
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      header
      sectionLabel("PROMPT")
      promptEditor
      sectionLabel("WHEN")
      idleTrigger
      loopOptions
      rules
      footer
    }
    .padding(EdgeInsets(top: 14, leading: 16, bottom: 16, trailing: 16))
    .frame(width: 400).foregroundStyle(MacTheme.ink)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
    .onAppear(perform: load)
  }

  private var header: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Menu {
        Button("Worker") { role = .worker }
        Button("Lead") { role = .chief }
      } label: {
        HStack(spacing: MacTheme.Space.x8) {
          Image(systemName: "repeat").font(.system(size: 15, weight: .medium))
            .foregroundStyle(MacTheme.accent)
          Text("Loop this pane")
            .font(MacTheme.chrome(14, weight: .bold)).foregroundStyle(MacTheme.ink)
        }
      }
      .menuIndicator(.hidden).buttonStyle(.plain)
      .help("Pane role: \(role == .chief ? "lead" : "worker")")
      Spacer()
      Button {
        enabled.toggle()
      } label: {
        LoopToggle(isOn: enabled)
      }
      .buttonStyle(.plain).accessibilityLabel("Run a task loop")
      .accessibilityValue(enabled ? "On" : "Off")
    }
  }

  private var promptEditor: some View {
    TextEditor(text: $prompt).font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .medium))
      .lineSpacing(4).scrollContentBackground(.hidden).padding(10).frame(height: 88)
      .neoWell(radius: MacTheme.Radius.control)
  }

  private var idleTrigger: some View {
    HStack(spacing: MacTheme.Space.x2) {
      Text("Every interval").foregroundStyle(MacTheme.muted)
        .frame(maxWidth: .infinity).padding(.vertical, MacTheme.Space.x6)
      Text("When idle").foregroundStyle(MacTheme.ink)
        .frame(maxWidth: .infinity).padding(.vertical, MacTheme.Space.x6)
        .neoRaised(radius: MacTheme.Radius.pill, elevation: .toggle)
    }
    .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
    .padding(MacTheme.Space.x3).background(MacTheme.well, in: Capsule())
    .accessibilityElement(children: .ignore)
    .accessibilityLabel("The loop sends only when the pane is idle")
  }

  private var loopOptions: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Menu {
        ForEach(intervalChoices, id: \.self) { value in
          Button(intervalLabel(value)) { interval = value }
        }
      } label: {
        optionLabel(icon: "timer", text: intervalLabel(interval))
      }
      Menu {
        ForEach(roundChoices, id: \.self) { value in
          Button(roundLabel(value)) { maxRounds = value }
        }
      } label: {
        optionLabel(icon: "number", text: roundLabel(maxRounds))
      }
      Menu {
        Button("Three empty results") { stopWhen = .three_empty }
        Button("Round limit") { stopWhen = .round_limit }
        Button("Until turned off") { stopWhen = .explicit_off }
      } label: {
        optionLabel(icon: "calendar", text: stopLabel)
      }
    }
    .menuIndicator(.hidden).buttonStyle(.plain).fixedSize(horizontal: false, vertical: true)
  }

  private var rules: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x5) {
      rule("Never sends over your draft.")
      rule("Busy ticks are skipped and recorded.")
      rule("Survives an engine restart.")
    }.padding(EdgeInsets(top: 4, leading: 2, bottom: 0, trailing: 2))
  }

  private var footer: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Text(loopStatus)
        .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .medium))
        .foregroundStyle(MacTheme.muted).lineLimit(1)
      Spacer(minLength: 0)
      Button("Cancel", action: dismiss).buttonStyle(.plain)
      Button("Apply", action: apply).buttonStyle(.plain)
        .disabled(enabled && prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
    }
    .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
    .foregroundStyle(MacTheme.accent).padding(.top, MacTheme.Space.x6)
  }

  private func sectionLabel(_ value: String) -> some View {
    Text(value).pickerHeading()
  }

  private func optionLabel(icon: String, text: String) -> some View {
    HStack(spacing: MacTheme.Space.x5) {
      Image(systemName: icon).font(.system(size: 12, weight: .medium))
      Text(text).font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
      Image(systemName: "chevron.down").font(.system(size: 11, weight: .medium))
        .foregroundStyle(MacTheme.muted)
    }
    .foregroundStyle(MacTheme.secondary).padding(.horizontal, 10).padding(.vertical, 6)
    .neoRaised(radius: MacTheme.Radius.pill, elevation: .chip)
  }

  private func rule(_ value: String) -> some View {
    HStack(alignment: .top, spacing: MacTheme.Space.x6) {
      Image(systemName: "checkmark").font(.system(size: 12, weight: .medium))
        .foregroundStyle(MacTheme.success)
      Text(value).font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .medium))
        .foregroundStyle(MacTheme.secondary)
    }
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

  private var intervalChoices: [UInt32] { [5, 10, 30, 60, 300, 600, 1_800, 3_600] }
  private var roundChoices: [UInt32] { [0, 1, 5, 10, 25, 50, 100] }

  private func intervalLabel(_ seconds: UInt32) -> String {
    if seconds < 60 { return "\(seconds) sec" }
    return "\(seconds / 60) min"
  }

  private func roundLabel(_ rounds: UInt32) -> String {
    rounds == 0 ? "no limit" : "\(rounds) rounds"
  }

  private var stopLabel: String {
    switch stopWhen {
    case .three_empty: "three empty"
    case .round_limit: "round limit"
    case .explicit_off: "until off"
    }
  }

  private var loopStatus: String {
    let rounds = client.paneLoops[pane.paneId]?.rounds ?? 0
    return enabled ? "next: when \(pane.name) is idle · \(rounds) today" : "off · \(rounds) today"
  }
}

private struct LoopToggle: View {
  let isOn: Bool

  var body: some View {
    HStack(spacing: 0) {
      if isOn { Spacer(minLength: 0) }
      Circle().fill(MacTheme.onAccent).frame(width: 18, height: 18)
        .shadow(color: MacTheme.shadowLight, radius: 2, x: -2, y: -2)
        .shadow(color: MacTheme.shadowDark, radius: 2, x: 2, y: 2)
      if !isOn { Spacer(minLength: 0) }
    }
    .padding(MacTheme.Space.x2).frame(width: 40, height: 22)
    .background(isOn ? MacTheme.accent : MacTheme.well, in: Capsule())
  }
}
