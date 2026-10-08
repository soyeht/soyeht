import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The approved composer's control row (`DCzvR`) follows what each harness advertises.
private enum Harness {
  static let claudeModes = ConfigOption(
    id: "mode", name: "Mode", category: "mode", type: .select, currentValue: .v0("acceptEdits"),
    options: [
      ConfigChoice(value: "default", name: "Manual"),
      ConfigChoice(value: "acceptEdits", name: "Accept edits"),
      ConfigChoice(value: "plan", name: "Plan"),
      ConfigChoice(value: "bypassPermissions", name: "Bypass", kind: "full_access"),
    ])
  static let claudeModel = ConfigOption(
    id: "model", name: "Model", category: "model", type: .select, currentValue: .v0("opus"),
    options: [
      ConfigChoice(value: "opus", name: "Opus 5 (1M)"),
      ConfigChoice(value: "sonnet", name: "Sonnet"),
    ])
  static let codexModel = ConfigOption(
    id: "model", name: "Model", category: "model", type: .select,
    currentValue: .v0("gpt-6-astra"),
    options: [ConfigChoice(value: "gpt-6-astra", name: "gpt-6-astra")])
  static let codexEffort = ConfigOption(
    id: "reasoning_effort", name: "Effort", category: "thought_level", type: .select,
    currentValue: .v0("medium"),
    options: ["low", "medium", "high", "xhigh"].map { ConfigChoice(value: $0, name: $0) })
  static let codexModes = ConfigOption(
    id: "mode", name: "Approval", category: "mode", type: .select, currentValue: .v0("ask"),
    options: [
      ConfigChoice(value: "ask", name: "Ask for approval"),
      ConfigChoice(value: "auto", name: "Approve for me"),
      ConfigChoice(value: "full", name: "Full access", kind: "full_access"),
    ])

  static func session(
    _ harness: String, _ options: [ConfigOption], operation: ConfigOperation? = nil
  )
    -> AgentSession
  {
    AgentSession(
      harness: harness, version: "1", fingerprint: "f", options: options, authMethods: [],
      operation: operation, queue: [], draft: false, role: .worker)
  }

  static func controls(
    _ agent: AgentSession?, state: PaneState = .idle, draft: Bool = false,
    connected: Bool = true, usage: TokenUsage? = nil, name: String = "Claude"
  ) -> AgentComposerControls {
    AgentComposerControls(
      harnessName: name, agent: agent, state: state, usage: usage, hasDraft: draft,
      connected: connected)
  }
}

@Test func claudeShowsModelAndPermissionButHidesTheEffortItDoesNotAdvertise() throws {
  let controls = Harness.controls(
    Harness.session("registry.claude-acp", [Harness.claudeModes, Harness.claudeModel]))

  #expect(try #require(controls.model).label == "Opus 5 (1M)")
  #expect(controls.effort == nil)
  #expect(try #require(controls.permission).currentMode?.name == "Accept edits")
  #expect(controls.showsThinkingSeparator)
  #expect(controls.showsPermissionSeparator)
  #expect(controls.placeholder == "Message Claude…")
}

@Test func codexShowsItsOwnModelEffortAndApprovalModes() throws {
  let controls = Harness.controls(
    Harness.session(
      "registry.codex-acp", [Harness.codexModel, Harness.codexEffort, Harness.codexModes]),
    name: "Codex")

  #expect(try #require(controls.model).choices.map(\.id) == ["gpt-6-astra"])
  let effort = try #require(controls.effort)
  #expect(effort.label == "medium")
  #expect(effort.choices.map(\.name) == ["low", "medium", "high", "xhigh"])
  #expect(effort.configId == "reasoning_effort")
  #expect(
    try #require(controls.permission).modes.map(\.name) == [
      "Ask for approval", "Approve for me", "Full access",
    ])
}

@Test func aHarnessThatAdvertisesNothingKeepsOnlyTheFixedControls() {
  let controls = Harness.controls(Harness.session("custom.tool", []), name: "Tool")

  #expect(controls.model == nil)
  #expect(controls.effort == nil)
  #expect(controls.permission == nil)
  #expect(!controls.showsThinkingSeparator)
  #expect(!controls.showsPermissionSeparator)
  #expect(controls.action == .send(enabled: false))
}

@Test func composerOptionsLeaveOtherOptionsToTheConfigurationRow() {
  let fast = ConfigOption(
    id: "fast", name: "Fast", category: nil, type: .boolean, currentValue: .v1(true), options: [])
  #expect(AgentComposerControls.isComposerOption(Harness.claudeModes))
  #expect(AgentComposerControls.isComposerOption(Harness.claudeModel))
  #expect(AgentComposerControls.isComposerOption(Harness.codexEffort))
  #expect(!AgentComposerControls.isComposerOption(fast))
}

@Test func aPendingChoiceShowsWhileTheHarnessAppliesItAndLocksTheMenus() throws {
  let operation = ConfigOperation(
    operationId: "op", configId: "model", value: .v0("sonnet"), startedAt: "2026-10-03T00:00:00Z")
  let controls = Harness.controls(
    Harness.session("registry.claude-acp", [Harness.claudeModel], operation: operation))

  let model = try #require(controls.model)
  #expect(model.pending == "sonnet")
  #expect(model.label == "Sonnet")
  #expect(!controls.configurable)
}

@Test func sendIsEnabledOnlyWithADraftAndAConnection() {
  let agent = Harness.session("registry.claude-acp", [])
  #expect(Harness.controls(agent).action == .send(enabled: false))
  #expect(Harness.controls(agent, draft: true).action == .send(enabled: true))
  #expect(Harness.controls(agent, draft: true, connected: false).action == .send(enabled: false))
  #expect(!Harness.controls(agent, draft: true).queuesOnSubmit)
}

@Test func aRunningAgentTurnsSendIntoStopAndTypingQueues() {
  let agent = Harness.session("registry.codex-acp", [])
  for state in [PaneState.working, .waiting_for_you, .no_signal] {
    let controls = Harness.controls(agent, state: state, draft: true, name: "Codex")
    #expect(controls.action == .stop(enabled: true), "\(state)")
    #expect(controls.queuesOnSubmit, "\(state)")
    #expect(controls.placeholder == "Queue a message for Codex…", "\(state)")
  }
  #expect(Harness.controls(agent, state: .cancelling).action == .stop(enabled: false))
  #expect(
    Harness.controls(agent, state: .working, connected: false).action == .stop(enabled: false))
  for state in [PaneState.idle, .starting, .dead] {
    #expect(!Harness.controls(agent, state: state).queuesOnSubmit, "\(state)")
  }
}

@Test func contextRingReadsTheLatestUsage() {
  let agent = Harness.session("registry.codex-acp", [])
  let usage = TokenUsage(totalTokens: "36000", contextSize: "200000")
  let controls = Harness.controls(agent, usage: usage)
  #expect(controls.contextFraction == 0.18)
  #expect(controls.contextPercentage == "18%")
  #expect(Harness.controls(agent, usage: TokenUsage(totalTokens: "10")).contextFraction == nil)
}

@Test func harnessLogosFollowTheProviderInTheLaunchId() {
  #expect(UsageProvider(harnessId: "registry.claude-acp") == .claude)
  #expect(UsageProvider(harnessId: "registry.codex-acp") == .codex)
  #expect(UsageProvider(harnessId: "registry.cursor") == .cursor)
  #expect(UsageProvider(harnessId: "local.my-tool") == nil)
}
