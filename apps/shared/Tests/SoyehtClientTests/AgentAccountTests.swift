import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private let signIn = AuthMethod(id: "login", name: "Claude Subscription", terminal: true)
private let console = AuthMethod(id: "console", name: "Anthropic Console", terminal: true)

private func agent(
  authMethods: [AuthMethod] = [], login: AgentLoginProgress? = nil,
  reauthMethods: [AuthMethod]? = nil, canSignOut: Bool? = nil, operation: ConfigOperation? = nil
) -> AgentSession {
  AgentSession(
    harness: "claude", version: "1", fingerprint: "fingerprint", sessionId: "session",
    options: [], authMethods: authMethods, operation: operation, queue: [], draft: false,
    role: .worker, login: login, reauthMethods: reauthMethods, canSignOut: canSignOut)
}

private func pane(_ id: String, agent: AgentSession?, state: PaneState = .idle) -> Pane {
  Pane(
    paneId: id, workspaceId: "workspace", name: "Claude", cwd: "/tmp", mode: .agent,
    state: state, source: .protocol, confidence: 1, phrase: "Ready", generation: 1, cols: 80,
    rows: 24, record: true, agent: agent)
}

@Test func signInCardOffersTheMethodsWhenTheAgentNeedsASignIn() {
  let state = AgentSignInState(agent: agent(), authMethods: [signIn, console], failure: nil)
  guard case .choose(let methods, let failure) = state else {
    Issue.record("expected the choice of methods, got \(state)")
    return
  }
  #expect(methods.map(\.id) == ["login", "console"])
  #expect(methods.allSatisfy { $0.opensTerminal })
  #expect(failure == nil)
  #expect(state.title == "Sign in to continue")
  #expect(!state.isWorking)
}

@Test func signInCardKeepsTheReasonTheLastAttemptDidNotFinish() {
  let reason = "Sign-in stopped because Soyeht's background services restarted. Sign in again."
  let state = AgentSignInState(agent: agent(), authMethods: [signIn], failure: reason)
  guard case .choose(_, let failure) = state else {
    Issue.record("expected the choice of methods, got \(state)")
    return
  }
  #expect(failure == reason)
}

@Test func aRunningSignInShowsItsTerminalInsteadOfTheChoice() {
  let running = agent(
    authMethods: [signIn], login: AgentLoginProgress(phase: .waiting, terminalPaneId: "terminal"))
  let state = AgentSignInState(agent: running, authMethods: [signIn], failure: "old failure")
  #expect(state == .waiting(terminalPaneId: "terminal"))
  #expect(state.title == "Finish signing in")
  #expect(state.detail?.contains("browser") == true)
  #expect(!state.isWorking)
}

@Test func theCardShowsProgressWhileTheAgentChecksTheNewAccountOrSignsOut() {
  let verifying = agent(login: AgentLoginProgress(phase: .verifying, terminalPaneId: "terminal"))
  let state = AgentSignInState(agent: verifying, authMethods: [], failure: nil)
  #expect(state == .verifying)
  #expect(state.isWorking)
  #expect(state.detail == "Restarting the agent. The conversation stays.")

  let signingOut = agent(login: AgentLoginProgress(phase: .signing_out))
  #expect(AgentSignInState(agent: signingOut, authMethods: [], failure: nil) == .signingOut)
  #expect(AgentSignInState(agent: signingOut, authMethods: [], failure: nil).isWorking)
}

@Test func aFailedAccountChangeOnASignedInPaneIsShownUntilDismissed() {
  let state = AgentSignInState(agent: agent(), authMethods: [], failure: "Sign-in was cancelled.")
  #expect(state == .failed(reason: "Sign-in was cancelled."))
  #expect(state.title == "Sign-in did not finish")
  #expect(state.detail == "Sign-in was cancelled.")
  #expect(AgentSignInState(agent: agent(), authMethods: [], failure: nil) == .hidden)
  #expect(AgentSignInState(agent: nil, authMethods: [], failure: nil) == .hidden)
}

@Test func aSignedInAgentOffersEachSignInAndSignOutWhenItCan() throws {
  let offered = agent(reauthMethods: [signIn, console], canSignOut: true)
  let actions = try #require(
    AgentAccountActions(agent: offered, paneState: .idle, connected: true))
  #expect(actions.switchMethods.map(\.name) == ["Claude Subscription", "Anthropic Console"])
  #expect(actions.canSignOut)
  #expect(actions.isEnabled)

  let onlySwitch = try #require(
    AgentAccountActions(agent: agent(reauthMethods: [signIn]), paneState: .idle, connected: true))
  #expect(!onlySwitch.canSignOut)
  #expect(
    AgentAccountActions(agent: agent(), paneState: .idle, connected: true) == nil,
    "an agent that has not signed in has no account to change")
  #expect(AgentAccountActions(agent: nil, paneState: nil, connected: true) == nil)
}

@Test func oneListOfAccountItemsServesEveryClient() throws {
  let both = try #require(
    AgentAccountActions(
      agent: agent(reauthMethods: [signIn, console], canSignOut: true), paneState: .idle,
      connected: true))
  #expect(
    both.items.map(\.title) == [
      "Switch account: Claude Subscription", "Switch account: Anthropic Console", "Sign out",
    ])
  #expect(
    both.items.map(\.action) == [
      .switchTo(methodId: "login"), .switchTo(methodId: "console"), .signOut,
    ])
  #expect(both.items.map(\.isDestructive) == [false, false, true])
  #expect(Set(both.items.map(\.id)).count == 3)

  let single = try #require(
    AgentAccountActions(
      agent: agent(reauthMethods: [signIn]), paneState: .idle, connected: true))
  #expect(single.items.map(\.title) == ["Switch account"])
  #expect(single.items.first?.accessibilityIdentifier == "agent-switch-account-login")
}

@Test func accountChangesAreDisabledWhileATurnRunsOrTheClientIsOffline() throws {
  let offered = agent(reauthMethods: [signIn], canSignOut: true)
  for state in [PaneState.working, .waiting_for_you, .cancelling, .no_signal] {
    let actions = try #require(
      AgentAccountActions(agent: offered, paneState: state, connected: true))
    #expect(!actions.isEnabled, "\(state)")
  }
  #expect(
    try #require(AgentAccountActions(agent: offered, paneState: .idle, connected: false))
      .isEnabled == false)
  let configuring = agent(
    reauthMethods: [signIn],
    operation: ConfigOperation(
      operationId: "operation", configId: "model", value: .v0("x"),
      startedAt: "2026-10-07T00:00:00Z"))
  #expect(
    try #require(AgentAccountActions(agent: configuring, paneState: .idle, connected: true))
      .isEnabled == false)
}

@MainActor @Test func theCardFollowsTheEnginesLoginFromSnapshotToFinish() throws {
  let client = EngineClient(socket: "/tmp/sn-agent-account/engine.sock")
  let paneId = "pane-agent"
  func state() -> AgentSignInState {
    let view = client.agentPanes[paneId] ?? AgentPaneView()
    return AgentSignInState(
      agent: client.panes.first { $0.paneId == paneId }?.agent, authMethods: view.authMethods,
      failure: view.signInFailure)
  }

  let signedOut = pane(paneId, agent: agent(authMethods: [signIn]))
  try client.applyAgentSnapshot([:], panes: [signedOut])
  client.upsertPane(signedOut)
  guard case .choose = state() else {
    Issue.record("expected the choice of methods, got \(state())")
    return
  }

  let waiting = pane(
    paneId,
    agent: agent(
      authMethods: [signIn],
      login: AgentLoginProgress(phase: .waiting, terminalPaneId: "terminal")))
  try client.applyAgentSnapshot([:], panes: [waiting])
  client.upsertPane(waiting)
  #expect(state() == .waiting(terminalPaneId: "terminal"))

  try client.applyAgentEvent(
    .paneAuthFinished,
    .encoded(
      PaneAuthFinishedEvent(
        eventId: "1", at: "2026-10-07T00:00:00Z", paneId: paneId, ok: false,
        reason: "Sign-in was cancelled.")))
  #expect(client.agentPanes[paneId]?.signInFailure == "Sign-in was cancelled.")
  #expect(client.agentPanes[paneId]?.notice == nil, "the failure belongs to the card")

  client.upsertPane(signedOut)
  guard case .choose(_, let failure) = state() else {
    Issue.record("expected the choice of methods with the failure, got \(state())")
    return
  }
  #expect(failure == "Sign-in was cancelled.")

  client.startAgentAuthentication(paneId, methodId: "login")
  #expect(client.agentPanes[paneId]?.signInFailure == nil)

  try client.applyAgentEvent(
    .paneAuthFinished,
    .encoded(
      PaneAuthFinishedEvent(
        eventId: "2", at: "2026-10-07T00:00:01Z", paneId: paneId, ok: true,
        reason: "Sign-in completed")))
  client.upsertPane(pane(paneId, agent: agent()))
  #expect(state() == .hidden)
}

@MainActor @Test func dismissingTheFailureHidesTheCard() {
  let client = EngineClient(socket: "/tmp/sn-agent-account-dismiss/engine.sock")
  var view = AgentPaneView()
  view.signInFailure = "Sign-in failed or was interrupted."
  client.agentPanes["pane-agent"] = view
  client.dismissSignInFailure("pane-agent")
  #expect(client.agentPanes["pane-agent"]?.signInFailure == nil)
}

@Test func reconnectWaitsLongerAfterEachRefusalUpToACeilingAndStartsOverAfterAConnection() {
  var backoff = ReconnectBackoff()
  let waits = (0..<8).map { _ in backoff.next() }
  #expect(
    waits == [
      .seconds(1), .seconds(2), .seconds(4), .seconds(8), .seconds(16), .seconds(30), .seconds(30),
      .seconds(30),
    ])
  backoff.reset()
  #expect(backoff.next() == .seconds(1))
}
