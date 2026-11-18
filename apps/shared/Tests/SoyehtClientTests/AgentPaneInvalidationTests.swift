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

@MainActor @Test func aChunkForOnePaneRunsNoViewOfAnotherPane() {
  let client = twoPanes()
  let otherPane = fires(
    watching: { readPane(client, "b") }, whenChanging: { streamAChunk(client, to: "a") })
  let samePane = fires(
    watching: { readPane(client, "a") }, whenChanging: { streamAChunk(client, to: "a") })
  #expect(!otherPane)
  #expect(samePane)
}

@MainActor @Test func aChunkDoesNotMoveWhatTheTabBarAndTheRailRead() {
  let client = twoPanes()
  let chunkFires = fires(
    watching: { _ = client.agentAttention }, whenChanging: { streamAChunk(client, to: "a") })
  let requestFires = fires(
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
  #expect(!chunkFires)
  #expect(requestFires)
  #expect(client.agentAttention.totalWaitingCount == 1)
}

@MainActor @Test func theTranscriptIsGroupedOncePerChangeAndNeverForAPaneNobodyReads() throws {
  let client = twoPanes()
  let model = try #require(client.agentPanes.model("a"))
  let hidden = try #require(client.agentPanes.model("b"))
  for _ in 0..<5 { _ = model.entries }
  #expect(model.groupings == 1)

  for index in 1...20 {
    try client.applyAgentEvent(.paneUpdate, chunk("a", "streaming \(index)"))
    try client.applyAgentEvent(.paneUpdate, chunk("b", "streaming \(index)"))
    client.partials.flush()
    for _ in 0..<3 { _ = model.entries }
  }
  #expect(model.groupings == 21)
  #expect(hidden.groupings == 0)
  #expect(model.entries.count == 51)
}

@MainActor @Test func aChunkForAHiddenPaneRunsNoBodyOfTheWindowChrome() {
  let client = twoPanes()
  client.upsertWorkspace(Workspace(workspaceId: "w", name: "w", path: "/tmp", layout: .null))
  // What each view of the chrome reads: the workspace tab and the rail button (the attention
  // summary and the workspaces), the usage ring (the rail's readings) and the pane card header
  // (whether its pane asks).
  let reads: [(String, @MainActor () -> Void)] = [
    (
      "workspace tab",
      {
        _ = client.agentAttention.waitingCount(in: "w")
        _ = client.agentAttention.hasWorkingAgent(in: "w")
        _ = client.workspaces
      }
    ),
    (
      "rail button",
      {
        _ = client.agentAttention.totalWaitingCount
        _ = client.agentAttention.hasWorkingAgent
      }
    ),
    ("usage ring", { _ = client.usage.readings }),
    ("pane card header of b", { _ = client.agentPanes.isAsking("b") }),
    ("pane card header of a", { _ = client.agentPanes.isAsking("a") }),
  ]
  for (name, read) in reads {
    let reached = fires(watching: read, whenChanging: { streamAChunk(client, to: "a") })
    #expect(!reached, "a chunk reached the \(name)")
  }
}

@MainActor private func askPermission(_ client: EngineClient, in paneId: String) {
  try? client.applyAgentEvent(
    .panePermissionRequested,
    .encoded(
      PanePermissionRequestedEvent(
        eventId: "2", at: "2026-11-18T00:00:01Z", paneId: paneId, requestId: "r", generation: 1,
        promptId: "p", title: "Write", detail: "", presentation: nil, options: [],
        expiresAt: "later")))
}

@MainActor @Test func aRequestMovesTheHeaderOfItsPaneAndNoOther() {
  let other = twoPanes()
  let otherFires = fires(
    watching: { _ = other.agentPanes.isAsking("b") },
    whenChanging: { askPermission(other, in: "a") })
  #expect(!otherFires)

  let same = twoPanes()
  let sameFires = fires(
    watching: { _ = same.agentPanes.isAsking("a") },
    whenChanging: { askPermission(same, in: "a") })
  #expect(sameFires)
  #expect(same.agentPanes.isAsking("a"))
  #expect(!same.agentPanes.isAsking("b"))
}
