import Foundation
import SoyehtProtocol
import SwiftUI
import Testing

@testable import SoyehtClient

/// A long reply arrives as thousands of partial updates, each carrying the whole text so far.
/// These tests bound the work by what is touched (messages written, replies parsed, rows
/// invalidated), not by wall time.

private let streamingPane = "pane-streaming"
private let streamingMessage = "reply"

private func reply(_ text: String, complete: Bool = false) -> TranscriptMessage {
  TranscriptMessage(
    localId: streamingMessage, generation: 1, seq: "900", role: .assistant, kind: .message,
    text: text, complete: complete)
}

private func history(_ count: Int) -> [TranscriptMessage] {
  (1...count).map { index in
    TranscriptMessage(
      localId: "m\(index)", generation: 1, seq: "\(index)",
      role: index.isMultiple(of: 2) ? .assistant : .user, kind: .message,
      text: "message \(index)", complete: true)
  }
}

private func event(_ text: String, complete: Bool = false) throws -> JsonValue {
  try .encoded(
    PaneUpdateEvent.v0(
      AgentMessageUpdate(
        eventId: "1", at: "2026-10-18T00:00:00Z", paneId: streamingPane,
        kind: .agent_message_chunk, generation: 1, seq: "900", localId: streamingMessage,
        text: text, complete: complete)))
}

/// The text of a reply that counts from 1 to `count`, one number per line.
private func counting(to count: Int) -> String {
  (1...count).map(String.init).joined(separator: "\n")
}

@Test func onlyTheNewestPartialUpdateOfAMessageSurvivesTheFrame() {
  var updates = AgentPartialUpdates()
  var firsts = 0
  for count in 1...2_000 {
    if updates.offer(paneId: streamingPane, reply(counting(to: count / 10 + 1))) { firsts += 1 }
  }
  #expect(firsts == 1)
  #expect(updates.superseded == 1_999)
  let batch = updates.drain()
  #expect(batch.count == 1)
  #expect(batch.first?.message.text == counting(to: 2_000 / 10 + 1))
  #expect(updates.isEmpty)
}

@Test func partialUpdatesOfDifferentMessagesKeepTheirArrivalOrder() {
  var updates = AgentPartialUpdates()
  _ = updates.offer(paneId: "a", reply("one"))
  var other = reply("other")
  other.localId = "second"
  _ = updates.offer(paneId: "a", other)
  _ = updates.offer(paneId: "b", reply("elsewhere"))
  _ = updates.offer(paneId: "a", reply("one, longer"))
  let batch = updates.drain()
  #expect(batch.map(\.message.localId) == [streamingMessage, "second", streamingMessage])
  #expect(batch.map(\.paneId) == ["a", "a", "b"])
  #expect(batch.first?.message.text == "one, longer")
}

@Test func onlyAReplyThatIsStillArrivingIsHeldForTheFrame() {
  #expect(AgentPartialUpdates.isPartial(reply("hel")))
  #expect(!AgentPartialUpdates.isPartial(reply("hello", complete: true)))
  var tool = reply("")
  tool.kind = .tool
  #expect(!AgentPartialUpdates.isPartial(tool))
  var thought = reply("hmm")
  thought.kind = .thought
  #expect(AgentPartialUpdates.isPartial(thought))
}

@Test func aStreamedReplyIsAlwaysMergedAtTheEndWhateverTheTranscriptLength() {
  var view = AgentPaneView()
  view.merge(history(500))
  let before = view.revisions
  var costs: [AgentPaneView.MergeCost] = []
  for count in 1...2_000 { costs.append(view.merge(reply(counting(to: count / 50 + 1)))) }
  #expect(costs.first == .appended)
  #expect(costs.dropFirst().allSatisfy { $0 == .replacedLast })
  #expect(view.messages.count == 501)
  // Entries copied: one. Every other message kept the revision it had.
  let changed = view.revisions.filter { before[$0.key] != $0.value }
  #expect(changed.keys.sorted() == [streamingMessage])
}

@Test func anOlderMessageStillMergesInOrderAndGetsANewRevision() {
  var view = AgentPaneView()
  view.merge(history(5))
  view.merge(reply("streaming"))
  let before = view.revision(of: "m2")
  var edited = history(5)[1]
  edited.text = "edited"
  #expect(view.merge(edited) == .searched)
  #expect(view.messages.map(\.localId) == ["m1", "m2", "m3", "m4", "m5", streamingMessage])
  #expect(view.messages[1].text == "edited")
  #expect(view.revision(of: "m2") > before)
}

@Test func aTranscriptPageIsMergedInOrderAndEveryMessageHasARevision() {
  var view = AgentPaneView()
  view.merge(Array(history(300).suffix(100)))
  view.merge(Array(history(300).prefix(100)))
  #expect(view.messages.count == 200)
  #expect(
    view.messages.map(\.localId)
      == view.messages.sorted { Int($0.seq)! < Int($1.seq)! }.map(\.localId))
  #expect(view.revisions.count == 200)
  #expect(Set(view.revisions.values).count == 200)
}

@Test func aRevisionIsNeverReusedAfterTheTranscriptIsReplaced() {
  var view = AgentPaneView()
  view.merge(reply("first"))
  let old = view.revision(of: streamingMessage)
  let page = PaneTranscriptResult(items: [reply("reloaded")], nextCursor: nil)
  view.mergeTranscriptPage(page, replacing: true)
  #expect(view.revision(of: streamingMessage) != old)
  #expect(view.messages.first?.text == "reloaded")
}

@MainActor @Test func twoThousandPartialUpdatesCostOneWritePerFrameAndOneParsePerWrite() throws {
  let client = EngineClient(socket: "/tmp/sn-agent-streaming/engine.sock")
  var view = AgentPaneView()
  view.merge(history(300))
  client.agentPanes[streamingPane] = view
  let before = client.agentPanes[streamingPane]?.revisions ?? [:]
  let parsesBefore = MarkdownReplyCache.parseCount

  let updates = 2_000
  let perFrame = 16
  for count in 1...updates {
    try client.applyAgentEvent(.paneUpdate, try event(counting(to: count / 10 + 1)))
    // Nothing is written between frames.
    #expect(client.partials.deliveries == (count - 1) / perFrame)
    if count % perFrame == 0 {
      client.partials.flush()
      // The row of the changed message parses its text; the other 300 rows do not run.
      let text = try #require(client.agentPanes[streamingPane]?.messages.last?.text)
      _ = MarkdownReplyCache.blocks(for: text, streaming: true)
    }
  }
  client.partials.flush()

  let frames = updates / perFrame + 1
  #expect(client.partials.deliveries <= frames)
  #expect(MarkdownReplyCache.parseCount - parsesBefore <= frames)
  let after = try #require(client.agentPanes[streamingPane]?.revisions)
  #expect(after.filter { before[$0.key] != $0.value }.keys.sorted() == [streamingMessage])
  #expect(client.agentPanes[streamingPane]?.messages.last?.text == counting(to: 2_000 / 10 + 1))
}

@MainActor @Test func aLaterEventSeesTheTranscriptAsTheEngineLeftIt() throws {
  let client = EngineClient(socket: "/tmp/sn-agent-streaming/engine.sock")
  try client.applyAgentEvent(.paneUpdate, try event("hel"))
  #expect(client.agentPanes[streamingPane] == nil)
  try client.applyAgentEvent(.paneUpdate, try event("hello"))
  // The finished message goes in, and the partial before it does not come back over it.
  try client.applyAgentEvent(.paneUpdate, try event("hello world", complete: true))
  #expect(client.agentPanes[streamingPane]?.messages.count == 1)
  #expect(client.agentPanes[streamingPane]?.messages.first?.text == "hello world")
  client.partials.flush()
  #expect(client.agentPanes[streamingPane]?.messages.first?.text == "hello world")
  #expect(client.partials.deliveries == 1)

  try client.applyAgentEvent(.paneUpdate, try event("pending text", complete: false))
  try client.applyAgentEvent(
    .paneQueuePaused,
    .encoded(
      PaneQueueStateEvent(
        eventId: "2", at: "2026-10-18T00:00:01Z", paneId: streamingPane, paused: true,
        reason: "interrupted")))
  #expect(client.agentPanes[streamingPane]?.messages.first?.text == "pending text")
  #expect(client.agentPanes[streamingPane]?.queuePaused == true)
}

@MainActor @Test func aPartialUpdateIsWrittenWithinAFrameWithoutAFlush() async throws {
  let client = EngineClient(socket: "/tmp/sn-agent-streaming/engine.sock")
  try client.applyAgentEvent(.paneUpdate, try event("arriving"))
  #expect(client.agentPanes[streamingPane] == nil)
  for _ in 0..<200 where client.agentPanes[streamingPane] == nil {
    try await Task.sleep(for: .milliseconds(ClientPolicy.agentPartialFlushMs))
  }
  #expect(client.agentPanes[streamingPane]?.messages.first?.text == "arriving")
}

@MainActor @Test func aClosedPaneDoesNotComeBackFromItsPendingText() throws {
  let client = EngineClient(socket: "/tmp/sn-agent-streaming/engine.sock")
  client.agentPanes[streamingPane] = AgentPaneView()
  try client.applyAgentEvent(.paneUpdate, try event("last words"))
  client.discardAgentPane(streamingPane)
  client.partials.flush()
  #expect(client.agentPanes[streamingPane]?.messages.first?.text == nil)
}

@MainActor @Test func aRowIsRedrawnOnlyWhenItsMessageRevisionMoves() {
  let style = AgentConversationStyle.pane(
    SoyehtThemeColorSet(palette: TerminalColorTheme.neoMilk.palette))
  let message = reply("text")
  func row(_ revision: UInt64, _ workspace: String = "w") -> AgentTranscriptRow {
    AgentTranscriptRow(message: message, revision: revision, workspaceId: workspace, style: style)
  }
  #expect(row(1) == row(1))
  #expect(row(1) != row(2))
  #expect(row(1) != row(1, "other"))
}

@Test func aParagraphIsSplitAtItsLineBreaksAndSpansKeepTheirStyle() {
  let runs = [
    MarkdownRun("one\ntwo "), MarkdownRun("bold\nstill bold", bold: true), MarkdownRun(" end"),
  ]
  #expect(
    MarkdownReply.lines(of: runs) == [
      [MarkdownRun("one")],
      [MarkdownRun("two "), MarkdownRun("bold", bold: true)],
      [MarkdownRun("still bold", bold: true), MarkdownRun(" end")],
    ])
  #expect(MarkdownReply.lines(of: [MarkdownRun("single")]) == [[MarkdownRun("single")]])
  #expect(MarkdownReply.lines(of: [MarkdownRun("a\n")]) == [[MarkdownRun("a")]])
  #expect(MarkdownReply.lines(of: []).isEmpty)
}

@Test func theLinesOfACountedReplyAreTheLinesTheEngineSent() {
  let blocks = MarkdownReply.parse(counting(to: 200), streaming: true)
  guard case .paragraph(let runs)? = blocks.first else {
    Issue.record("a counted reply is one paragraph")
    return
  }
  #expect(MarkdownReply.lines(of: runs).count == 200)
  #expect(MarkdownReply.lines(of: runs).last == [MarkdownRun("200")])
}

@MainActor @Test func linksStayInertWithoutAWorkspaceAndOpenWithOne() {
  let style = AgentConversationStyle.pane(
    SoyehtThemeColorSet(palette: TerminalColorTheme.neoMilk.palette))
  let runs = [MarkdownRun("docs", link: "https://example.com/docs")]
  let preview = MarkdownLook(workspaceId: nil, style: style, ink: style.ink)
  let pane = MarkdownLook(workspaceId: "w", style: style, ink: style.ink)
  // The editor's preview has no workspace: its links look like links and open nothing.
  let inert = preview.attributed(runs)
  #expect(inert.runs.allSatisfy { $0.link == nil })
  #expect(inert.runs.allSatisfy { $0.foregroundColor == style.accent })
  #expect(inert.runs.allSatisfy { $0.underlineStyle == .single })
  #expect(pane.attributed(runs).runs.allSatisfy { $0.link != nil })
  #expect(preview != pane)
}

@Test func aRevisionIsNeverReusedWhenTheViewIsDroppedAndMadeAgain() {
  var first = AgentPaneView()
  first.merge(reply("one"))
  first.merge(reply("two"))
  let seen = first.revision(of: streamingMessage)
  // A paired device clears `agentPanes` when it disconnects; the next transcript is a new view.
  var second = AgentPaneView()
  second.merge(reply("one"))
  #expect(second.revision(of: streamingMessage) > seen)
}

@MainActor @Test func aDisconnectWritesTheWaitingTextBeforeItDropsAnything() throws {
  let client = EngineClient(socket: "/tmp/sn-agent-streaming/engine.sock")
  try client.applyAgentEvent(.paneUpdate, try event("last words"))
  #expect(client.agentPanes[streamingPane] == nil)
  client.resetAgentSession()
  #expect(client.agentPanes[streamingPane]?.messages.first?.text == "last words")
  #expect(client.partials.deliveries == 1)
  // Nothing is left to arrive after the connection is gone.
  client.agentPanes = [:]
  client.partials.flush()
  #expect(client.agentPanes[streamingPane] == nil)
}
