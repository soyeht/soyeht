import Foundation
import Observation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// An agent works in a pane nobody looks at. Its replies, its turns and its state must cost the
/// client next to nothing and must not run what shows another pane. The tests count what is
/// written and what is notified, not time, so they say the same on any machine.

/// Counts how many times what `read` reads changed, re-arming after each one.
@MainActor private final class Changes {
  private(set) var count = 0

  init(_ read: @escaping @MainActor () -> Void) { arm(read) }

  private func arm(_ read: @escaping @MainActor () -> Void) {
    withObservationTracking(read) {
      MainActor.assumeIsolated {
        self.count += 1
        self.arm(read)
      }
    }
  }
}

/// The views of the tests, kept for as long as the tests run: a pane is shown while its viewer lives.
@MainActor private var viewers: [AgentPaneViewer] = []

/// Shows `paneId` the way a mounted conversation view does.
@MainActor func showPane(_ client: EngineClient, _ paneId: String) {
  let viewer = AgentPaneViewer()
  viewers.append(viewer)
  client.agentPaneShown(viewer, paneId, true)
}

/// Answers the reads of a pane's transcript with `page` (given the request's parameters) and the
/// queue as empty.
@MainActor private func stubTranscript(
  _ client: EngineClient,
  _ page: @escaping @MainActor (JsonValue) async throws -> PaneTranscriptResult
) {
  client.requestStub = { method, params in
    switch method {
    case .paneTranscript: return try .encoded(await page(params))
    case .paneQueueList: return try .encoded(PaneQueueListResult(items: [], draft: false))
    default: throw ClientFailure.invalidFrame
    }
  }
}

private func history(_ id: String) -> [TranscriptMessage] {
  (1...50).map { index in
    TranscriptMessage(
      localId: "\(id)-\(index)", generation: 1, seq: "\(index)", role: .assistant,
      kind: .message, text: "message \(index)", complete: true)
  }
}

private func chunk(_ paneId: String, _ text: String, complete: Bool = false) throws -> JsonValue {
  try .encoded(
    PaneUpdateEvent.v0(
      AgentMessageUpdate(
        eventId: "1", at: "2026-11-19T00:00:00Z", paneId: paneId, kind: .agent_message_chunk,
        generation: 1, seq: "900", localId: "reply-\(paneId)", text: text, complete: complete)))
}

private func stateChanged(_ paneId: String, _ state: PaneState) throws -> JsonValue {
  try .encoded(
    PaneStateChangedEvent(
      eventId: "2", at: "2026-11-19T00:00:00Z", paneId: paneId, state: state,
      confidence: 1, phrase: "\(state)", source: .protocol))
}

/// A client showing the agent pane `v` of workspace `w1`, and holding `hidden` agent panes of
/// workspace `w2` that no view shows. Every pane holds a transcript.
@MainActor private func client(hidden count: Int) -> (EngineClient, [String]) {
  let client = EngineClient(socket: "/tmp/sn-hidden-panes/engine.sock")
  client.connected = true
  let ids = (0..<count).map { "h\($0)" }
  client.panes =
    [agentPane("v", workspaceId: "w1")]
    + ids.map { agentPane($0, workspaceId: "w2", state: .working) }
  for id in ["v"] + ids { client.agentPanes[id, default: AgentPaneView()].merge(history(id)) }
  showPane(client, "v")
  return (client, ids)
}

@MainActor @Test func aReplyStreamedInHiddenPanesIsNeitherDecodedNorWritten() throws {
  let (client, hidden) = client(hidden: 6)
  let writes = hidden.map { id in Changes { _ = client.agentPanes[id] } }
  // 6 agents, 200 updates each, a frame's flush between: what a busy afternoon sends.
  for round in 1...200 {
    for id in hidden { try client.applyAgentEvent(.paneUpdate, chunk(id, "reply \(round)")) }
    if round.isMultiple(of: 3) { client.partials.flush() }
  }
  #expect(writes.map(\.count) == Array(repeating: 0, count: 6), "a hidden pane was written")
  #expect(client.partials.offered == 0, "an update of a hidden pane waited for a frame")
  #expect(hidden.allSatisfy { client.agentPanes.isStale($0) }, "the reload on show is marked")
}

@MainActor @Test func aHiddenAgentRunsNoReaderOfAnotherPaneAndOnlyAChangedSummary() throws {
  let (client, hidden) = client(hidden: 3)
  let visible = Changes {
    _ = client.agentPanes.model("v")?.entries
    _ = client.pane("v")
    _ = client.panes(in: "w1")
    _ = client.agentPanes.isAsking("v")
  }
  let tabs = Changes { _ = client.agentAttention }
  let workspace = Changes { _ = client.panes(in: "w2") }
  for id in hidden {
    for round in 1...100 { try client.applyAgentEvent(.paneUpdate, chunk(id, "reply \(round)")) }
    // A turn of the agent: its state is set again each time it changes the pane.
    for _ in 1...5 { try client.applyPaneEvent(.paneStateChanged, stateChanged(id, .working)) }
  }
  #expect(visible.count == 0, "what shows pane v ran for work in another workspace")
  #expect(tabs.count == 0, "the summary did not change: the agents were working already")
  #expect(workspace.count == 15, "each write of a pane tells its own workspace, once")

  for id in hidden { try client.applyPaneEvent(.paneStateChanged, stateChanged(id, .idle)) }
  #expect(visible.count == 0)
  #expect(tabs.count == 3, "each agent that stops is one change of the summary, not five writes")
}

@MainActor @Test func aPaneThatChangesRunsTheReadersOfThatPaneAndWorkspace() throws {
  let (client, _) = client(hidden: 1)
  let own = Changes { _ = client.pane("v") }
  let sameWorkspace = Changes { _ = client.panes(in: "w1") }
  let list = Changes { _ = client.panes }
  try client.applyPaneEvent(.paneStateChanged, stateChanged("v", .working))
  #expect(own.count == 1 && sameWorkspace.count == 1 && list.count == 1)
  #expect(client.pane("v")?.state == .working && client.panes(in: "w1").first?.state == .working)
  client.removePane("v")
  #expect(client.pane("v") == nil && client.panes(in: "w1").isEmpty && client.panes.count == 1)
}

/// What a person sees when they open a pane in which an agent has been working: the whole
/// reply, as the reload brings it, and then the live updates that follow.
@MainActor @Test func aPaneHiddenDuringAReplyShowsTheWholeReplyWhenShown() async throws {
  let (client, hidden) = client(hidden: 1)
  let id = hidden[0]
  let reply = TranscriptMessage(
    localId: "reply-\(id)", generation: 1, seq: "900", role: .assistant, kind: .message,
    text: "the whole reply, which streamed while the pane was hidden", complete: true)
  var reads = 0
  stubTranscript(client) { _ in
    reads += 1
    return PaneTranscriptResult(items: [reply], nextCursor: nil)
  }
  for text in ["the", "the whole", "the whole reply, which"] {
    try client.applyAgentEvent(.paneUpdate, chunk(id, text))
  }
  #expect(client.agentPanes[id]?.messages.contains { $0.localId == reply.localId } == false)

  showPane(client, id)
  for _ in 0..<200 where client.agentPanes[id]?.messages.last?.text != reply.text {
    await Task.yield()
  }
  #expect(reads == 1)
  #expect(client.agentPanes[id]?.messages.last?.text == reply.text)

  // From here the pane is followed message by message again.
  try client.applyAgentEvent(.paneUpdate, chunk(id, "and one more", complete: false))
  client.partials.flush()
  #expect(client.partials.offered == 1)
}

// What a pane that was hidden must still hold when it is shown: nothing the growing text skipped
// may leave a hole or an open row behind.

private func message(_ index: Int, text: String? = nil, complete: Bool = true) -> TranscriptMessage
{
  TranscriptMessage(
    localId: "m-\(index)", generation: 1, seq: "\(index)", role: .assistant, kind: .message,
    text: text ?? "message \(index)", complete: complete)
}

private func update(_ paneId: String, _ index: Int, text: String? = nil, complete: Bool = true)
  throws -> JsonValue
{
  try .encoded(
    PaneUpdateEvent.v0(
      AgentMessageUpdate(
        eventId: "1", at: "2026-11-19T00:00:00Z", paneId: paneId, kind: .agent_message_chunk,
        generation: 1, seq: "\(index)", localId: "m-\(index)", text: text ?? "message \(index)",
        complete: complete)))
}

private func subagentRow(_ paneId: String, status: SubagentStatus) throws -> JsonValue {
  try .encoded(
    PaneUpdateEvent.v8(
      SubagentUpdate(
        eventId: "1", at: "2026-11-19T00:00:00Z", paneId: paneId, kind: .subagent,
        generation: 1, seq: "2", localId: "row",
        subagent: SubagentRun(subagentId: "alpha", name: "Alpha", task: "Task", status: status))))
}

/// A client with the agent pane `paneId`, and the view that shows it.
@MainActor private func shownPane(_ paneId: String) -> (EngineClient, AgentPaneViewer) {
  let client = EngineClient(socket: "/tmp/sn-hidden-panes/engine.sock")
  client.connected = true
  client.panes = [agentPane(paneId, workspaceId: "w")]
  let viewer = AgentPaneViewer()
  client.agentPaneShown(viewer, paneId, true)
  return (client, viewer)
}

@MainActor private func settle(_ done: @MainActor () -> Bool) async {
  for _ in 0..<2000 where !done() { await Task.yield() }
}

@MainActor @Test func aLongRunInAHiddenPaneLeavesNoHoleWhenItIsShown() async throws {
  let (client, viewer) = shownPane("h")
  for index in 1...50 { try client.applyAgentEvent(.paneUpdate, update("h", index)) }
  client.agentPaneShown(viewer, "h", false)
  // Finished messages are applied while hidden; the window keeps the newest.
  for index in 51...450 { try client.applyAgentEvent(.paneUpdate, update("h", index)) }
  stubTranscript(client) { params in
    let before = params["before"].string.flatMap { Int($0) } ?? 451
    let first = max(1, before - 100)
    return PaneTranscriptResult(
      items: (first..<before).map { message($0) }, nextCursor: first > 1 ? "\(first)" : nil)
  }
  client.agentPaneShown(viewer, "h", true)
  await settle { client.agentPanes["h"]?.messages.last?.seq == "450" }
  let seqs = (client.agentPanes["h"]?.messages ?? []).compactMap { Int($0.seq) }
  #expect(seqs.last == 450)
  #expect(zip(seqs, seqs.dropFirst()).allSatisfy { $1 == $0 + 1 }, "the transcript skips messages")
}

@MainActor @Test func aRowThatFinishedWhileHiddenIsNotLeftRunning() async throws {
  let (client, viewer) = shownPane("h")
  try client.applyAgentEvent(.paneUpdate, subagentRow("h", status: .running))
  for index in 3...160 { try client.applyAgentEvent(.paneUpdate, update("h", index)) }
  client.agentPaneShown(viewer, "h", false)
  for index in 161...180 { try client.applyAgentEvent(.paneUpdate, update("h", index)) }
  try client.applyAgentEvent(.paneUpdate, subagentRow("h", status: .completed))
  stubTranscript(client) { _ in
    PaneTranscriptResult(items: (81...180).map { message($0) }, nextCursor: "81")
  }
  client.agentPaneShown(viewer, "h", true)
  await settle { client.agentPanes["h"]?.messages.last?.seq == "180" }
  let row = client.agentPanes["h"]?.messages.first { $0.localId == "row" }
  #expect(row == nil || row?.subagent?.status == .completed, "a helper that finished shows running")
}

@MainActor @Test func aPaneShownMidReplyKeepsTheNewerLiveText() async throws {
  let (client, viewer) = shownPane("h")
  client.agentPaneShown(viewer, "h", false)
  try client.applyAgentEvent(.paneUpdate, update("h", 7, text: "a", complete: false))
  #expect(client.agentPanes["h"] == nil && client.agentPanes.isStale("h"))
  var release: CheckedContinuation<Void, Never>?
  stubTranscript(client) { _ in
    await withCheckedContinuation { release = $0 }
    return PaneTranscriptResult(items: [message(7, text: "a b", complete: false)], nextCursor: nil)
  }
  client.agentPaneShown(viewer, "h", true)
  await settle { release != nil }
  try client.applyAgentEvent(.paneUpdate, update("h", 7, text: "a b c d", complete: false))
  client.partials.flush()
  release?.resume()
  await settle { !client.agentPanes.isStale("h") }
  #expect(client.agentPanes["h"]?.messages.map(\.text) == ["a b c d"])
}

@MainActor @Test func aClosedPaneLeavesNothingInTheStores() throws {
  let (client, hidden) = client(hidden: 1)
  try client.applyAgentEvent(.paneUpdate, chunk(hidden[0], "text"))
  #expect(client.agentPanes.isStale(hidden[0]))
  client.removePane(hidden[0])
  #expect(!client.agentPanes.isStale(hidden[0]) && client.pane(hidden[0]) == nil)
}

@Test func onlyTheGrowingTextOfAReplyOrAThoughtIsSkippedOnTheWire() throws {
  func wire(_ update: PaneUpdateEvent) throws -> JsonValue { try .encoded(update) }
  let partial = try wire(
    .v0(
      AgentMessageUpdate(
        eventId: "1", at: "t", paneId: "p", kind: .agent_message_chunk, generation: 1, seq: "1",
        localId: "m", text: "x", complete: false)))
  let finished = try wire(
    .v0(
      AgentMessageUpdate(
        eventId: "1", at: "t", paneId: "p", kind: .agent_message_chunk, generation: 1, seq: "1",
        localId: "m", text: "x", complete: true)))
  let thought = try wire(
    .v1(
      AgentThoughtUpdate(
        eventId: "1", at: "t", paneId: "p", kind: .agent_thought_chunk, generation: 1, seq: "1",
        localId: "m", text: "x", complete: false)))
  let tool = try subagentRow("p", status: .running)
  #expect(
    AgentPartialUpdates.isPartial(wire: partial) && AgentPartialUpdates.isPartial(wire: thought))
  #expect(
    !AgentPartialUpdates.isPartial(wire: finished) && !AgentPartialUpdates.isPartial(wire: tool))
}
