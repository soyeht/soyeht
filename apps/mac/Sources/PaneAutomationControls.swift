import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct PaneAutomationPanel: View {
  @Bindable var client: EngineClient
  let pane: Pane
  let dismiss: () -> Void
  @State private var role = AgentPaneRole.worker
  @State private var prompt = LoopPresentation.defaultPrompt
  @State private var interval = LoopPresentation.defaultInterval
  @State private var maxRounds = LoopPresentation.defaultMaxRounds
  @State private var stopWhen = LoopPresentation.defaultStopWhen
  @State private var enabled = false

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      header
      sectionLabel(LoopPresentation.promptHeading)
      promptEditor
      sectionLabel(LoopPresentation.whenHeading)
      idleTrigger
      loopOptions
      rules
      footer
    }
    .padding(EdgeInsets(top: 14, leading: 16, bottom: 16, trailing: 16))
    .frame(width: DesignTokens.Layout.loopPanelWidth).foregroundStyle(MacTheme.ink)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
    .onAppear(perform: load)
  }

  private var header: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Menu {
        Button(LoopPresentation.workerRole) { role = .worker }
        Button(LoopPresentation.leadRole) { role = .chief }
      } label: {
        HStack(spacing: MacTheme.Space.x8) {
          Image(systemName: PaneHeaderAction.loop.systemImage).font(
            .system(size: 15, weight: .medium)
          )
          .foregroundStyle(MacTheme.accent)
          Text(LoopPresentation.title)
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
      Text(LoopPresentation.everyInterval).foregroundStyle(MacTheme.muted)
        .frame(maxWidth: .infinity).padding(.vertical, MacTheme.Space.x6)
      Text(LoopPresentation.whenIdle).foregroundStyle(MacTheme.ink)
        .frame(maxWidth: .infinity).padding(.vertical, MacTheme.Space.x6)
        .neoRaised(radius: MacTheme.Radius.pill, elevation: .toggle)
    }
    .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .semibold))
    .padding(MacTheme.Space.x3).background(MacTheme.well, in: Capsule())
    .accessibilityElement(children: .ignore)
    .accessibilityLabel(LoopPresentation.idleExplanation)
  }

  private var loopOptions: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Menu {
        ForEach(LoopPresentation.intervalChoices, id: \.self) { value in
          Button(LoopPresentation.intervalLabel(value)) { interval = value }
        }
      } label: {
        optionLabel(icon: "timer", text: LoopPresentation.intervalLabel(interval))
      }
      Menu {
        ForEach(LoopPresentation.roundChoices, id: \.self) { value in
          Button(LoopPresentation.roundLabel(value)) { maxRounds = value }
        }
      } label: {
        optionLabel(icon: "number", text: LoopPresentation.roundLabel(maxRounds))
      }
      Menu {
        ForEach(LoopPresentation.stopChoices, id: \.self) { value in
          Button(LoopPresentation.stopChoiceTitle(value)) { stopWhen = value }
        }
      } label: {
        optionLabel(icon: "calendar", text: LoopPresentation.stopLabel(stopWhen))
      }
    }
    .menuIndicator(.hidden).buttonStyle(.plain).fixedSize(horizontal: false, vertical: true)
  }

  private var rules: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x5) {
      ForEach(LoopPresentation.rules, id: \.self) { rule($0) }
    }.padding(EdgeInsets(top: 4, leading: 2, bottom: 0, trailing: 2))
  }

  private var footer: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Text(loopStatus)
        .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .medium))
        .foregroundStyle(MacTheme.muted).lineLimit(1)
      Spacer(minLength: 0)
      Button(LoopPresentation.cancelTitle, action: dismiss).buttonStyle(.plain)
      Button(LoopPresentation.applyTitle, action: apply).buttonStyle(.plain)
        .disabled(!LoopPresentation.canApply(enabled: enabled, prompt: prompt))
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
    let start = LoopPresentation.initial(client.paneLoops[pane.paneId])
    (prompt, interval, maxRounds, stopWhen, enabled) = start
  }

  private func apply() {
    if role != pane.agent?.role { client.setPaneRole(pane.paneId, role: role) }
    client.setLoop(
      paneId: pane.paneId, prompt: prompt, interval: interval, maxRounds: maxRounds,
      stopWhen: stopWhen, enabled: enabled)
    dismiss()
  }

  private var loopStatus: String {
    LoopPresentation.status(
      enabled: enabled, paneName: pane.name, rounds: client.paneLoops[pane.paneId]?.rounds ?? 0)
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
