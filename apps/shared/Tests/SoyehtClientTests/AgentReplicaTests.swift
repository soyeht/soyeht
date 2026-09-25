import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func agentStatusShowsSignInInsteadOfStartingWhenAuthenticationIsRequired() {
  #expect(
    AgentPaneStatusPresentation.title(
      state: .starting, needsAuthentication: true, hasSession: true) == "Sign in")
  #expect(
    AgentPaneStatusPresentation.title(
      state: .starting, needsAuthentication: false, hasSession: false)
      == "starting")
  #expect(
    AgentPaneStatusPresentation.title(
      state: .starting, needsAuthentication: false, hasSession: true) == "ready")
  #expect(
    AgentPaneStatusPresentation.title(
      state: .idle, needsAuthentication: false, hasSession: true) == "idle")
}

@Test func visibleAgentTranscriptHydrationIncludesNewlyShownPaneWithoutDuplicates() {
  func pane(_ id: String, mode: PaneMode, workspaceId: String = "workspace") -> Pane {
    Pane(
      paneId: id, workspaceId: workspaceId, name: id, cwd: "/tmp", mode: mode,
      state: .idle, source: .protocol, confidence: 1, phrase: "Ready", generation: 1,
      cols: 80, rows: 24, record: mode == .agent)
  }
  let panes = [
    pane("codex", mode: .agent), pane("opencode", mode: .agent),
    pane("shell", mode: .terminal),
    pane("other-workspace", mode: .agent, workspaceId: "another-workspace"),
  ]
  let split = [
    PresentedPane(paneId: "codex", isVisible: true),
    PresentedPane(paneId: "opencode", isVisible: true),
    PresentedPane(paneId: "shell", isVisible: true),
    PresentedPane(paneId: "codex", isVisible: true),
  ]
  #expect(
    AgentTranscriptHydrationPolicy.paneIdsToLoad(
      presented: split, panes: panes, hydrated: [], loading: []) == ["codex", "opencode"])

  // The companion tab stays mounted but hidden until selected.
  let tabs = [
    PresentedPane(paneId: "codex", isVisible: true),
    PresentedPane(paneId: "opencode", isVisible: false),
  ]
  #expect(
    AgentTranscriptHydrationPolicy.paneIdsToLoad(
      presented: tabs, panes: panes, hydrated: ["codex"], loading: []) == [])
  #expect(
    AgentTranscriptHydrationPolicy.paneIdsToLoad(
      presented: [PresentedPane(paneId: "opencode", isVisible: true)], panes: panes,
      hydrated: ["codex"], loading: []) == ["opencode"])
  #expect(
    AgentTranscriptHydrationPolicy.paneIdsToLoad(
      presented: split, panes: panes, hydrated: ["codex"], loading: ["opencode"]) == [])
  // A new connection has no hydrated IDs, so both visible transcripts reload.
  #expect(
    AgentTranscriptHydrationPolicy.paneIdsToLoad(
      presented: split, panes: panes, hydrated: [], loading: []) == ["codex", "opencode"])
}

@MainActor @Test func agentReplicaReplacesChunksTracksQuestionsAndResetsSnapshotState() throws {
  let client = EngineClient(socket: "/tmp/sn-agent-replica/engine.sock")
  let paneId = "pane-agent"
  func update(text: String, complete: Bool) throws {
    try client.applyAgentEvent(
      .paneUpdate,
      .encoded(
        PaneUpdateEvent.v0(
          AgentMessageUpdate(
            eventId: "1", at: "2026-09-11T00:00:00Z", paneId: paneId,
            kind: .agent_message_chunk, generation: 1, seq: "7", localId: "message-1",
            text: text, complete: complete))))
  }

  try update(text: "hel", complete: false)
  try update(text: "hello", complete: true)
  #expect(client.agentPanes[paneId]?.messages.count == 1)
  #expect(client.agentPanes[paneId]?.messages.first?.text == "hello")
  #expect(client.agentPanes[paneId]?.messages.first?.complete == true)

  let option = PermissionOption(optionId: "allow", kind: .allow_once, name: "Allow")
  try client.applyAgentEvent(
    .panePermissionRequested,
    .encoded(
      PanePermissionRequestedEvent(
        eventId: "2", at: "2026-09-11T00:00:01Z", paneId: paneId,
        requestId: "request-1", generation: 1, promptId: "prompt-1", title: "Run",
        detail: "Run the requested operation", options: [option],
        expiresAt: "2026-09-11T00:30:01Z")))
  #expect(client.agentPanes[paneId]?.permission?.requestId == "request-1")

  try client.applyAgentEvent(
    .paneQueuePaused,
    .encoded(
      PaneQueueStateEvent(
        eventId: "3", at: "2026-09-11T00:00:02Z", paneId: paneId, paused: true,
        reason: "interrupted")))
  #expect(client.agentPanes[paneId]?.queuePaused == true)

  let authMethod = AuthMethod(id: "login", name: "Sign in", terminal: true)
  let session = AgentSession(
    harness: "agent", version: "1", fingerprint: "fingerprint", options: [],
    authMethods: [], permission: .ask, yolo: false, queue: [], draft: false,
    role: .worker,
    harnessSwitch: AgentHarnessSwitch(fromHarness: "first", toHarness: "second"))
  let pane = Pane(
    paneId: paneId, workspaceId: "workspace", name: "Agent", cwd: "/tmp",
    mode: .agent, state: .idle, source: .protocol, confidence: 1, phrase: "Ready",
    generation: 1, cols: 80, rows: 24, record: true, agent: session)
  try client.applyAgentSnapshot([:], panes: [pane])
  #expect(client.agentPanes[paneId]?.permission == nil)
  #expect(client.agentPanes[paneId]?.queuePaused == false)
  #expect(client.agentPanes[paneId]?.authMethods.isEmpty == true)
  #expect(client.agentPanes[paneId]?.harnessSwitch?.phase == .switching)
  #expect(client.agentPanes[paneId]?.harnessSwitch?.fromHarness == "first")
  #expect(client.agentPanes[paneId]?.harnessSwitch?.toHarness == "second")

  try client.applyAgentEvent(
    .paneNeedsLogin,
    .encoded(
      PaneNeedsLoginEvent(
        eventId: "4", at: "2026-09-11T00:00:03Z", paneId: paneId,
        harness: "agent", authMethods: [authMethod])))
  #expect(client.agentPanes[paneId]?.authMethods.first?.id == "login")
  #expect(client.agentPanes[paneId]?.authMethods.first?.terminal == true)

  try client.applyAgentSnapshot([:], panes: [pane])
  #expect(client.agentPanes[paneId]?.authMethods.isEmpty == true)
}

@Test func remoteFailuresHideWireMethodAndKindFromPeople() {
  let failure = ClientFailure.remote(
    "pane_busy", "This pane is not waiting for sign-in")
  #expect(failure.localizedDescription == "This pane is not waiting for sign-in")
}

@MainActor @Test func agentTranscriptPagingUsesTheServerCursor() throws {
  let client = EngineClient(socket: "/tmp/sn-agent-paging/engine.sock")
  let paneId = "pane-agent"
  let newest = TranscriptMessage(
    localId: "message-100", generation: 1, seq: "100", role: .assistant, kind: .message,
    text: "Newest", complete: true)
  var view = AgentPaneView()
  view.mergeTranscriptPage(
    PaneTranscriptResult(items: [newest], nextCursor: "37"), replacing: true)
  client.agentPanes[paneId] = view

  #expect(client.agentPanes[paneId]?.hasOlderMessages == true)
  #expect(client.olderTranscriptParams(paneId)?.before == "37")
  #expect(client.olderTranscriptParams(paneId)?.before != newest.seq)

  let oldest = TranscriptMessage(
    localId: "message-1", generation: 1, seq: "1", role: .user, kind: .message,
    text: "Oldest", complete: true)
  view.mergeTranscriptPage(PaneTranscriptResult(items: [oldest]), replacing: false)
  #expect(view.messages.map(\.seq) == ["1", "100"])
  #expect(view.hasOlderMessages == false)
}

@MainActor @Test func harnessSwitchEventsStateTheRestartAndPreserveTheFailureFallback() throws {
  let client = EngineClient(socket: "/tmp/sn-agent-switch-events/engine.sock")
  let paneId = "pane-agent"
  try client.applyAgentEvent(
    .paneHarnessSwitchStarted,
    .encoded(
      PaneHarnessSwitchStartedEvent(
        eventId: "1", at: "2026-09-15T00:00:00Z", paneId: paneId,
        fromHarness: "first", toHarness: "second")))
  #expect(client.agentPanes[paneId]?.harnessSwitch?.phase == .switching)

  try client.applyAgentEvent(
    .paneHarnessSwitchDone,
    .encoded(
      PaneHarnessSwitchDoneEvent(
        eventId: "2", at: "2026-09-15T00:00:01Z", paneId: paneId,
        fromHarness: "first", toHarness: "second", newSessionId: "new-session",
        markerSeq: "9")))
  #expect(client.agentPanes[paneId]?.harnessSwitch?.phase == .restarted)
  #expect(client.agentPanes[paneId]?.harnessSwitch?.toHarness == "second")

  try client.applyAgentEvent(
    .paneHarnessSwitchFailed,
    .encoded(
      PaneHarnessSwitchFailedEvent(
        eventId: "3", at: "2026-09-15T00:00:02Z", paneId: paneId,
        fromHarness: "second", toHarness: "third", reason: "Candidate stopped",
        restored: true)))
  #expect(client.agentPanes[paneId]?.harnessSwitch?.phase == .failed)
  #expect(client.agentPanes[paneId]?.harnessSwitch?.reason == "Candidate stopped")
  #expect(
    client.agentPanes[paneId]?.notice
      == "The switch failed. The original agent is still running.")
}

@MainActor @Test func remoteClientCannotStartAgentAuthentication() throws {
  let credential = DeviceCredential(
    engineId: "engine", deviceId: "device",
    address: try #require(URL(string: "ws://100.64.0.1:8291")),
    token: "token", key: Data(repeating: 0, count: 32))
  let client = EngineClient(credential: credential)

  client.startAgentAuthentication("pane-agent", methodId: "login")

  #expect(client.problem == "Finish agent sign-in on the Mac.")
}
