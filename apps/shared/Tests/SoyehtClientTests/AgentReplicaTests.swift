import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

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
    authMethods: [authMethod], permission: .ask, yolo: false, queue: [], draft: false)
  let pane = Pane(
    paneId: paneId, workspaceId: "workspace", name: "Agent", cwd: "/tmp",
    mode: .agent, state: .idle, source: .protocol, confidence: 1, phrase: "Ready",
    generation: 1, cols: 80, rows: 24, record: true, agent: session)
  try client.applyAgentSnapshot([:], panes: [pane])
  #expect(client.agentPanes[paneId]?.permission == nil)
  #expect(client.agentPanes[paneId]?.queuePaused == false)
  #expect(client.agentPanes[paneId]?.authMethods.count == 1)
  #expect(client.agentPanes[paneId]?.authMethods.first?.id == "login")
  #expect(client.agentPanes[paneId]?.authMethods.first?.terminal == true)
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

@MainActor @Test func remoteClientCannotStartAgentAuthentication() throws {
  let credential = DeviceCredential(
    engineId: "engine", deviceId: "device",
    address: try #require(URL(string: "ws://100.64.0.1:8291")),
    token: "token", key: Data(repeating: 0, count: 32))
  let client = EngineClient(credential: credential)

  client.startAgentAuthentication("pane-agent", methodId: "login")

  #expect(client.problem == "Finish agent sign-in on the Mac.")
}
