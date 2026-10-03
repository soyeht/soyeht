import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func switchSectionsLeaveOutTheRunningHarnessAndKeepPickerOrder() {
  let claude = switchHarness(id: "registry.claude-acp", name: "Claude Code", state: .ready)
  let codex = switchHarness(id: "registry.codex-acp", name: "Codex", state: .ready)
  let gemini = switchHarness(id: "registry.gemini", name: "Gemini", state: .sign_in)
  let missing = switchHarness(id: "registry.missing", name: "Absent", state: .not_installed)

  let layout = HarnessSwitchModel.layout(
    [missing, gemini, codex, claude], current: claude.id)

  #expect(layout.sections.map(\.group) == [.ready, .signIn])
  #expect(layout.sections.flatMap(\.items).map(\.id) == [codex.id, gemini.id])
  #expect(layout.more.map(\.id) == [missing.id])
  #expect(
    HarnessSwitchModel.layout([claude, codex], current: claude.id, query: "clau").isEmpty)
}

/// The switch menu of a 41-entry registry catalogue: the two installed harnesses lead,
/// and the rest wait under "More harnesses" in a stable order.
@Test func switchMenuCollapsesTheUninstalledCatalogueUnderMoreHarnesses() {
  let claude = switchHarness(id: "registry.claude-acp", name: "Claude", state: .ready)
  let codex = switchHarness(id: "registry.codex-acp", name: "Codex", state: .ready)
  let missing = (0..<39).map { index in
    switchHarness(id: "registry.agent-\(index)", name: "Agent \(index)", state: .not_installed)
  }
  let catalogue = missing + [codex, claude]

  let layout = HarnessSwitchModel.layout(catalogue, current: claude.id)

  #expect(layout.sections.flatMap(\.items).map(\.id) == [codex.id])
  #expect(layout.more.count == 39)
  #expect(layout.more.allSatisfy { !HarnessPickerModel.isInstalled($0) })
  #expect(
    HarnessSwitchModel.layout(catalogue.reversed(), current: claude.id).more.map(\.id)
      == layout.more.map(\.id))
  #expect(HarnessPickerLayout.moreTitle == "More harnesses")
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

@Test func switchLabelsNameTheTarget() {
  let codex = switchHarness(id: "codex", name: "Codex", state: .ready)

  #expect(HarnessSwitchModel.actionLabel(for: codex) == "Restart conversation with Codex")
  #expect(HarnessSwitchModel.actionLabel(for: nil) == "Choose another agent")
  #expect(HarnessSwitchModel.readableName(ofHarnessId: "custom.gone") == "Gone")
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

@Test func agentPaneHeaderExposesASwitchHarnessControlNamedAfterTheRunningHarness() throws {
  let claude = switchHarness(id: "registry.claude-acp", name: "Claude Agent", state: .ready)
  let codex = switchHarness(id: "registry.codex-acp", name: "Codex", state: .ready)
  let harness = HarnessSwitchModel.control(
    current: claude.id, switchState: nil, harnesses: [claude, codex], paneName: "Reviewer")

  let header = PaneHeaderControls(mode: .agent, harness: harness)

  let control = try #require(header.harness)
  #expect(control.title == "Claude")
  #expect(!control.busy)
  #expect(HarnessSwitchControl.accessibilityLabel == "Switch harness")
  #expect(control.help == "Switch harness (running Claude)")
  #expect(HarnessSwitchControl.symbol == "chevron.down")
}

@Test func harnessControlNamesTheTargetWhileASwitchRuns() {
  let claude = switchHarness(id: "registry.claude-acp", name: "Claude Agent", state: .ready)
  let codex = switchHarness(id: "registry.codex-acp", name: "Codex", state: .ready)
  let switching = AgentHarnessSwitchView(
    fromHarness: claude.id, toHarness: codex.id, phase: .switching)

  let running = HarnessSwitchModel.control(
    current: claude.id, switchState: switching, harnesses: [claude, codex],
    paneName: "Claude")
  #expect(running == HarnessSwitchControl(title: "Codex", busy: true))
  let requested = HarnessSwitchModel.control(
    current: claude.id, switchState: nil, requested: codex.id, harnesses: [claude, codex],
    paneName: "Claude")
  #expect(requested == HarnessSwitchControl(title: "Codex", busy: true))
}

/// VM1 regression: a pane launched as "registry.claude-acp" showed "registry.cla… ▾".
@Test func registryClaudePaneReadsClaudeAndNeverItsLaunchId() {
  let claude = switchHarness(id: "registry.claude-acp", name: "Claude", state: .ready)
  let codex = switchHarness(id: "registry.codex-acp", name: "Codex", state: .ready)

  // Catalogue loaded: the engine's display name from harness.list.
  #expect(
    HarnessSwitchModel.control(
      current: "registry.claude-acp", switchState: nil, harnesses: [claude, codex],
      paneName: "Reviewer"
    ).title == "Claude")
  // Catalogue not loaded yet: the pane's display name.
  #expect(
    HarnessSwitchModel.control(
      current: "registry.claude-acp", switchState: nil, harnesses: [], paneName: "Claude"
    ).title == "Claude")
  // Neither: a name read from the identifier, never the identifier itself.
  for paneName in [nil, "", "  "] as [String?] {
    #expect(
      HarnessSwitchModel.control(
        current: "registry.claude-acp", switchState: nil, harnesses: [], paneName: paneName
      ).title == "Claude")
  }
  let target = HarnessSwitchModel.control(
    current: "registry.claude-acp", switchState: nil, requested: "registry.codex-acp",
    harnesses: [], paneName: "Claude")
  #expect(target.title == "Codex")
  #expect(HarnessSwitchModel.readableName(ofHarnessId: "local.my_tool") == "My Tool")
  for id in ["registry.claude-acp", "registry.codex-acp", "local.tool"] {
    let title = HarnessSwitchModel.control(
      current: id, switchState: nil, harnesses: [], paneName: nil
    ).title
    #expect(!title.contains("registry.") && !title.contains("local.") && title != id)
  }
}

@Test func harnessShortNameDropsOnlyTheAdapterSuffix() {
  #expect(HarnessSwitchModel.shortName("Claude Agent") == "Claude")
  #expect(HarnessSwitchModel.shortName("Codex") == "Codex")
  #expect(HarnessSwitchModel.shortName(" Agent") == "Agent")
  #expect(HarnessSwitchModel.shortName("Test agent") == "Test agent")
}

@Test func paneHeaderIconsAreNamedAndLoopNoLongerLooksLikeSwitching() {
  let agent = PaneHeaderControls(
    mode: .agent, harness: HarnessSwitchControl(title: "Claude", busy: false))
  #expect(agent.actions == [.loop, .tasks, .splitColumns, .splitRows, .close])
  for action in PaneHeaderAction.allCases {
    #expect(!action.title.isEmpty)
    #expect(!action.accessibilityIdentifier(paneId: "p").isEmpty)
  }
  #expect(!PaneHeaderAction.loop.systemImage.contains("arrow"))
  #expect(PaneHeaderAction.loop.systemImage != "repeat")

  let terminal = PaneHeaderControls(
    mode: .terminal, harness: HarnessSwitchControl(title: "Claude", busy: false))
  #expect(terminal.harness == nil)
  #expect(terminal.actions == [.tasks, .splitColumns, .splitRows, .close])
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
