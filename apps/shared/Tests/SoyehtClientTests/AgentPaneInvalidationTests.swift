import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// A pane's views follow that pane alone. Each test reads what a view body reads and says whether
/// a change made the body run again, so no window is involved.

private func agentPane(_ id: String) -> Pane {
  Pane(
    paneId: id, workspaceId: "w", name: id, cwd: "/tmp", mode: .agent, state: .working,
    source: .protocol, confidence: 1, phrase: "", generation: 1, cols: 80, rows: 24, record: true)
}

/// A client with two streaming agent panes, `a` and `b`, each holding a long transcript.
@MainActor private func twoPanes() -> EngineClient {
  let client = EngineClient(socket: "/tmp/sn-pane-invalidation/engine.sock")
  client.connected = true
  for id in ["a", "b"] {
    client.upsertPane(agentPane(id))
    client.agentPanes[id, default: AgentPaneView()].merge(
      (1...50).map { index in
        TranscriptMessage(
          localId: "\(id)\(index)", generation: 1, seq: "\(index)", role: .assistant,
          kind: .message, text: "message \(index)", complete: true)
      })
  }
  return client
}

private func chunk(_ paneId: String, _ text: String) throws -> JsonValue {
  try .encoded(
    PaneUpdateEvent.v0(
      AgentMessageUpdate(
        eventId: "1", at: "2026-11-18T00:00:00Z", paneId: paneId, kind: .agent_message_chunk,
        generation: 1, seq: "900", localId: "reply-\(paneId)", text: text, complete: false)))
}

/// What the views of one pane read: its state, its grouped transcript, and what the tab bar reads.
@MainActor private func readPane(_ client: EngineClient, _ id: String) {
  _ = client.agentPanes[id]?.messages
  _ = client.agentPanes.model(id)?.entries
}

@MainActor private func streamAChunk(_ client: EngineClient, to id: String) {
  try? client.applyAgentEvent(.paneUpdate, chunk(id, "streaming"))
  client.partials.flush()
}

@MainActor @Test func aPermissionRequestMovesTheAttentionSummary() {
  let client = twoPanes()
  let moved = fires(
    watching: { _ = client.agentAttention },
    whenChanging: {
      try? client.applyAgentEvent(
        .panePermissionRequested,
        .encoded(
          PanePermissionRequestedEvent(
            eventId: "2", at: "2026-11-18T00:00:01Z", paneId: "a", requestId: "r",
            generation: 1, promptId: "p", title: "Write", detail: "", presentation: nil,
            options: [], expiresAt: "later")))
    })
  #expect(moved)
  #expect(client.agentPanes.isAsking("a") && !client.agentPanes.isAsking("b"))
}

@MainActor @Test func theTranscriptIsGroupedOncePerChangeAndNeverForAPaneNobodyReads() {
  var passes = 0
  let model = AgentPaneModel(group: { messages in
    passes += 1
    return AgentTranscriptGrouping.entries(messages)
  })
  func message(_ text: String) -> TranscriptMessage {
    TranscriptMessage(
      localId: "reply", generation: 1, seq: "9", role: .assistant, kind: .message, text: text,
      complete: false)
  }
  // Nobody reads the pane while it streams: nothing is grouped.
  for index in 1...20 { model.view.merge(message("streaming \(index)")) }
  #expect(passes == 0)

  for _ in 0..<5 { _ = model.entries }
  #expect(passes == 1)

  // A reader that looks at each change reads it three times and groups once.
  for index in 21...40 {
    model.view.merge(message("streaming \(index)"))
    for _ in 0..<3 { _ = model.entries }
  }
  #expect(passes == 21)
}

@MainActor @Test func aPaneIsShownWhileAnyViewShowsItAndAcrossAReconnect() {
  let store = AgentPaneStore()
  let (first, second) = (AgentPaneViewer(), AgentPaneViewer())
  store["p"] = AgentPaneView()
  #expect(store.setViewer(first, "p", shown: true))
  #expect(!store.setViewer(second, "p", shown: true), "a second view changes nothing")
  #expect(!store.setViewer(first, "p", shown: false) && store["p"]?.scroll.isShown == true)
  // A reconnect drops the panes while the mounted view never reports again.
  store.removeAll()
  store["p"] = AgentPaneView()
  #expect(store["p"]?.scroll.isShown == true)
  #expect(store.setViewer(second, "p", shown: false) && store["p"]?.scroll.isShown == false)
}

@MainActor @Test func aChunkForAHiddenPaneRunsNoBodyOfTheWindowChrome() {
  let client = twoPanes()
  client.upsertWorkspace(Workspace(workspaceId: "w", name: "w", path: "/tmp", layout: .null))
  // What each view of the chrome reads: the workspace tab and the rail button (the attention
  // summary and the workspaces), the usage ring (the rail's readings) and the pane card header
  // (whether its pane asks).
  let reads: [(String, @MainActor () -> Void)] = [
    ("views of pane b", { readPane(client, "b") }),
    (
      "workspace tab and rail button",
      {
        _ = client.agentAttention.waitingCount(in: "w")
        _ = client.agentAttention.hasWorkingAgent(in: "w")
        _ = client.workspaces
        _ = client.agentAttention.totalWaitingCount
        _ = client.agentAttention.hasWorkingAgent
      }
    ),
    ("usage ring", { _ = client.usage.readings }),
    ("pane card header of b", { _ = client.agentPanes.isAsking("b") }),
  ]
  for (name, read) in reads {
    let reached = fires(watching: read, whenChanging: { streamAChunk(client, to: "a") })
    #expect(!reached, "a chunk reached the \(name)")
  }
  // The pane that streams is the one whose views move.
  let own = fires(
    watching: { readPane(client, "a") }, whenChanging: { streamAChunk(client, to: "a") })
  #expect(own)
}

/// A pane no view shows is not read when a turn ends, and is read, once, when a view shows it; a
/// read that fails leaves the pane to be read the next time. The prompt of a turn reaches a client
/// only with that read.
@MainActor @Test func aTurnThatEndsOnAPaneNoViewShowsIsReadWhenItIsShown() async throws {
  let client = EngineClient(socket: "/tmp/sn-pane-invalidation/engine.sock")
  client.connected = true
  client.upsertPane(agentPane("a"))
  client.agentPanes["a"] = AgentPaneView()
  var reads = 0
  var failing = true
  let prompt = TranscriptMessage(
    localId: "prompt", generation: 1, seq: "1", role: .user, kind: .message, text: "hello",
    complete: true)
  client.requestStub = { method, _ in
    switch method {
    case .paneTranscript:
      reads += 1
      if failing { throw ClientFailure.remote("internal_error", "busy") }
      return try .encoded(PaneTranscriptResult(items: [prompt], nextCursor: nil))
    case .paneQueueList: return try .encoded(PaneQueueListResult(items: [], draft: false))
    default: throw ClientFailure.invalidFrame
    }
  }
  func settle(_ done: @MainActor () -> Bool) async {
    for _ in 0..<200 where !done() { await Task.yield() }
  }
  let ended = PaneTurnEndedEvent(
    eventId: "1", at: "2026-11-19T00:00:00Z", paneId: "a", promptId: "p", stopReason: .end_turn)
  try client.applyAgentEvent(.paneTurnEnded, .encoded(ended))
  await Task.yield()
  #expect(reads == 0, "a pane no view shows is not read when a turn ends")

  let viewer = AgentPaneViewer()
  client.agentPaneShown(viewer, "a", true)
  await settle { reads == 1 }
  await Task.yield()
  #expect(reads == 1 && client.agentPanes["a"]?.messages.isEmpty == true, "the read failed")

  client.agentPaneShown(viewer, "a", false)
  failing = false
  client.agentPaneShown(viewer, "a", true)
  await settle { client.agentPanes["a"]?.messages.isEmpty == false }
  #expect(reads == 2 && client.agentPanes["a"]?.messages.first?.text == "hello")

  client.agentPaneShown(viewer, "a", false)
  client.agentPaneShown(viewer, "a", true)
  await Task.yield()
  #expect(reads == 2, "what the read brought is not read again")
}
