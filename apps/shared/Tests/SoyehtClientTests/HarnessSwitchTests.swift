import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func switchSectionsLeaveOutTheRunningHarnessAndKeepPickerOrder() {
  let claude = switchHarness(id: "registry.claude-acp", name: "Claude Code", state: .ready)
  let codex = switchHarness(id: "registry.codex-acp", name: "Codex", state: .ready)
  let gemini = switchHarness(id: "registry.gemini", name: "Gemini", state: .sign_in)
  let missing = switchHarness(id: "registry.missing", name: "Absent", state: .not_installed)

  let sections = HarnessSwitchModel.sections(
    [missing, gemini, codex, claude], current: claude.id)

  #expect(sections.map(\.group) == [.ready, .signIn, .unavailable])
  #expect(sections.flatMap(\.items).map(\.id) == [codex.id, gemini.id, missing.id])
  #expect(
    HarnessSwitchModel.sections([claude, codex], current: claude.id, query: "clau").isEmpty)
}

@Test func switchDefaultTargetNeverPicksTheRunningHarness() {
  let claude = switchHarness(id: "claude", name: "Claude Code", state: .ready)
  let codex = switchHarness(id: "codex", name: "Codex", state: .ready)

  #expect(
    HarnessSwitchModel.defaultTarget(
      in: [claude, codex], current: claude.id, preferredId: claude.id)?.id == codex.id)
  #expect(
    HarnessSwitchModel.defaultTarget(in: [claude], current: claude.id, preferredId: nil)
      == nil)
}

@Test func switchLabelsNameTheTargetAndFallBackToTheIdentifier() {
  let codex = switchHarness(id: "codex", name: "Codex", state: .ready)

  #expect(HarnessSwitchModel.actionLabel(for: codex) == "Restart conversation with Codex")
  #expect(HarnessSwitchModel.actionLabel(for: nil) == "Choose another agent")
  #expect(HarnessSwitchModel.displayName(of: "codex", in: [codex]) == "Codex")
  #expect(HarnessSwitchModel.displayName(of: "custom.gone", in: [codex]) == "custom.gone")
}

@Test func switchIsBlockedWhileDisconnectedConfiguringOrAlreadySwitching() {
  let idle = agentSession(operation: nil)
  let configuring = agentSession(
    operation: ConfigOperation(
      operationId: "op", configId: "model", value: .v0("fast"), startedAt: "now"))
  let switching = AgentHarnessSwitchView(
    fromHarness: "claude", toHarness: "codex", phase: .switching)
  let failed = AgentHarnessSwitchView(
    fromHarness: "claude", toHarness: "codex", phase: .failed, reason: "boom")

  #expect(HarnessSwitchModel.canSwitch(connected: true, agent: idle, switchState: nil))
  #expect(HarnessSwitchModel.canSwitch(connected: true, agent: idle, switchState: failed))
  #expect(!HarnessSwitchModel.canSwitch(connected: false, agent: idle, switchState: nil))
  #expect(!HarnessSwitchModel.canSwitch(connected: true, agent: configuring, switchState: nil))
  #expect(!HarnessSwitchModel.canSwitch(connected: true, agent: idle, switchState: switching))
}

@MainActor @Test func switchRunReportsTheNewHarnessOrTheEngineReason() async {
  let codex = switchHarness(id: "codex", name: "Codex", state: .ready)
  let switched = await HarnessSwitchModel.run(target: codex) {
    PaneHarnessSwitchResult(
      paneId: "pane", fromHarness: "claude", toHarness: "codex", newSessionId: "s2",
      markerSeq: "7")
  }
  let refused = await HarnessSwitchModel.run(target: codex) {
    throw ClientFailure.remote("busy", "The pane is already switching.")
  }

  #expect(switched == .switched(toHarness: "codex"))
  #expect(refused == .failed("The pane is already switching."))
}

@MainActor @Test func switchRunRefusesAnUnavailableHarnessWithoutCallingTheEngine() async {
  let missing = switchHarness(id: "missing", name: "Missing", state: .not_installed)
  var called = false

  let outcome = await HarnessSwitchModel.run(target: missing) {
    called = true
    throw ClientFailure.disconnected
  }

  #expect(outcome == .unavailable)
  #expect(!called)
}

@MainActor @Test func clientBlocksSwitchingAPaneWhoseSwitchIsRunning() {
  let client = EngineClient(socket: "/tmp/harness-switch.sock")
  #expect(!client.canSwitchAgentHarness("pane"))
  client.agentPanes["pane"] = AgentPaneView()
  client.agentPanes["pane"]?.harnessSwitch = AgentHarnessSwitchView(
    fromHarness: "claude", toHarness: "codex", phase: .switching)
  #expect(HarnessSwitchModel.isSwitching(client.agentPanes["pane"]?.harnessSwitch))
}

private func agentSession(operation: ConfigOperation?) -> AgentSession {
  AgentSession(
    harness: "claude", version: "1", fingerprint: "f", options: [], authMethods: [],
    operation: operation, queue: [], draft: false, role: .worker)
}

private func switchHarness(id: String, name: String, state: HarnessSetupState) -> HarnessInfo {
  HarnessInfo(
    id: id, name: name, available: state != .not_installed, capabilities: [], options: [],
    authMethods: [], source: .built_in, setupState: state)
}
