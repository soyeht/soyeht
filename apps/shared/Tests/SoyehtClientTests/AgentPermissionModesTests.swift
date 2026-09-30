import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func modeOption(
  current: String, _ choices: [(String, String, String?)], id: String = "mode"
) -> ConfigOption {
  ConfigOption(
    id: id, name: "Mode", category: "mode", type: .select, currentValue: .v0(current),
    options: choices.map { ConfigChoice(value: $0.0, name: $0.1, kind: $0.2) })
}

private let codex: [(String, String, String?)] = [
  ("read-only", "Ask for approval", "standard"),
  ("agent", "Approve for me", "auto_review"),
  ("agent-full-access", "Full access", "full_access"),
]

private let claude: [(String, String, String?)] = [
  ("default", "Manual", "standard"),
  ("acceptEdits", "Accept edits", "standard"),
  ("plan", "Plan", "plan"),
  ("auto", "Auto", "auto_review"),
  ("bypassPermissions", "Bypass permissions", "full_access"),
]

@Test func shiftTabCyclesAnnouncedModesAndNeverReachesFullAccess() throws {
  func next(_ choices: [(String, String, String?)], from current: String) -> String? {
    AgentPermissionModes(options: [modeOption(current: current, choices)])?.nextInCycle?.id
  }
  #expect(next(codex, from: "read-only") == "agent")
  #expect(next(codex, from: "agent") == "read-only")
  #expect(next(codex, from: "agent-full-access") == "read-only")
  #expect(next(claude, from: "plan") == "auto")
  #expect(next(claude, from: "auto") == "default")
  #expect(next(claude, from: "unannounced") == "default")
  #expect(next([("only", "Only", "standard"), ("all", "All", "full_access")], from: "only") == nil)
}

@Test func menuShowsHarnessNamesAndHoldsThePendingModeApart() throws {
  let option = modeOption(current: "agent", codex)
  let pending = ConfigOperation(
    operationId: "operation", configId: "mode", value: .v0("read-only"), startedAt: "now")
  let modes = try #require(AgentPermissionModes(options: [option], operation: pending))
  #expect(modes.modes.map(\.name) == ["Ask for approval", "Approve for me", "Full access"])
  #expect(modes.modes.map(\.fullAccess) == [false, false, true])
  #expect(modes.currentMode?.name == "Approve for me")
  #expect(modes.pendingMode?.name == "Ask for approval")
  #expect(modes.nextInCycle == nil)
  let model = ConfigOperation(
    operationId: "operation", configId: "model", value: .v0("other"), startedAt: "now")
  #expect(AgentPermissionModes(options: [option], operation: model)?.pending == nil)
}

@Test func onlyTheHarnessModeSelectorBecomesTheModeMenu() {
  let model = ConfigOption(
    id: "model", name: "Model", category: "model", type: .select, currentValue: .v0("a"),
    options: [ConfigChoice(value: "a", name: "A")])
  #expect(AgentPermissionModes(options: [model]) == nil)
  #expect(AgentPermissionModes(options: [model, modeOption(current: "auto", claude)]) != nil)
  let modes = AgentPermissionModes(options: [modeOption(current: "auto", claude)])
  #expect(
    PermissionModeChoices.title(modes: modes, selection: nil, preferred: "acceptEdits")
      == "Accept edits")
  #expect(
    PermissionModeChoices.title(modes: modes, selection: "plan", preferred: "acceptEdits")
      == "Plan")
  #expect(
    PermissionModeChoices.title(modes: nil, selection: nil, preferred: "acceptEdits")
      == "Harness default")
}
