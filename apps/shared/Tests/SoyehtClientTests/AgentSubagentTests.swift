import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func message(
  _ seq: Int, _ text: String, owner: String? = nil, kind: TranscriptKind = .message,
  run: SubagentRun? = nil
) -> TranscriptMessage {
  TranscriptMessage(
    localId: "m\(seq)", generation: 1, seq: "\(seq)", role: .assistant, kind: kind,
    text: text, complete: true, subagent: run, subagentId: owner)
}

private func row(_ seq: Int, _ id: String, owner: String? = nil, status: SubagentStatus = .running)
  -> TranscriptMessage
{
  message(
    seq, "", owner: owner, kind: .subagent,
    run: SubagentRun(subagentId: id, name: id.capitalized, task: "Task \(id)", status: status))
}

/// The transcript entries as text: a message by its text, a subagent by its name and what it holds.
func shape(_ entries: [AgentTranscriptEntry]) -> [String] {
  entries.map { entry in
    switch entry {
    case .message(let message): message.text
    case .subagent(let group):
      "\(group.run?.name ?? "?")[\(shape(group.entries).joined(separator: ","))]"
    }
  }
}

@Test func subagentActivityNestsUnderItsRowInsteadOfTheParentTranscript() {
  let messages = [
    message(1, "delegating"),
    row(2, "alpha"),
    row(3, "beta"),
    message(4, "alpha tool", owner: "alpha"),
    message(5, "beta says", owner: "beta"),
    row(6, "gamma", owner: "alpha"),
    message(7, "gamma says", owner: "gamma"),
    message(8, "alpha says", owner: "alpha"),
    message(9, "both done"),
  ]
  #expect(
    shape(AgentTranscriptGrouping.entries(messages)) == [
      "delegating",
      "Alpha[alpha tool,Gamma[gamma says],alpha says]",
      "Beta[beta says]",
      "both done",
    ])
}

@Test func activityWhoseRowIsOnAnOlderPageStaysGroupedWhereItStarts() {
  let messages = [
    message(10, "orphan tool", owner: "early"),
    message(11, "parent"),
    message(12, "orphan says", owner: "early"),
  ]
  let entries = AgentTranscriptGrouping.entries(messages)
  #expect(shape(entries) == ["?[orphan tool,orphan says]", "parent"])
  #expect(entries.first?.id == "subagent:early")
}

@Test func subagentRowsStartClosedAndToggleIndependently() {
  var disclosure = AgentSubagentDisclosure()
  #expect(!disclosure.isExpanded("alpha"))
  disclosure.toggle("alpha")
  #expect(disclosure.isExpanded("alpha"))
  #expect(!disclosure.isExpanded("beta"))
  disclosure.toggle("beta")
  disclosure.toggle("alpha")
  #expect(!disclosure.isExpanded("alpha"))
  #expect(disclosure.isExpanded("beta"))
}

@Test func subagentRowShowsLiveElapsedThenOutcomeAndModel() throws {
  func presentation(_ run: SubagentRun, now: Date = Date()) -> AgentSubagentPresentation {
    AgentSubagentPresentation(
      AgentSubagentGroup(subagentId: run.subagentId, run: run, entries: []), now: now)
  }
  let start = try #require(UsageTimestamp.date(from: "2026-09-30T12:00:00Z"))
  var run = SubagentRun(
    subagentId: "alpha", name: "Explore", task: "Look", status: .running,
    startedAt: "2026-09-30T12:00:00Z")
  let running = presentation(run, now: start.addingTimeInterval(12))
  #expect(running.status == .running)
  #expect(running.trailing == "0m 12s")
  #expect(running.model == nil)

  run.status = .completed
  run.model = "claude-model"
  run.summary = "Found it"
  let done = presentation(run)
  #expect(done.status == .succeeded)
  #expect(done.trailing == "done")
  #expect(done.model == "claude-model")
  #expect(done.summary == "Found it")

  run.status = .failed
  #expect(presentation(run).status == .failed)
  run.status = .disconnected
  #expect(presentation(run).status == .failed)
  #expect(presentation(run).trailing == "disconnected")
  run.status = .cancelled
  #expect(presentation(run).status == .stopped)

  // A replayed run has no start time.
  run.status = .running
  run.startedAt = nil
  #expect(presentation(run).trailing == "running")
}

@MainActor @Test func liveSubagentUpdatesKeepTheirOwnerInTheReplica() throws {
  let client = EngineClient(socket: "/tmp/sn-agent-replica/engine.sock")
  let paneId = "pane-agent"
  let run = SubagentRun(subagentId: "child", name: "Explore", task: "Look", status: .running)
  try client.applyAgentEvent(
    .paneUpdate,
    .encoded(
      PaneUpdateEvent.v8(
        SubagentUpdate(
          eventId: "1", at: "2026-09-30T00:00:00Z", paneId: paneId, kind: .subagent,
          generation: 1, seq: "3", localId: "row", subagent: run))))
  try client.applyAgentEvent(
    .paneUpdate,
    .encoded(
      PaneUpdateEvent.v2(
        ToolCallUpdate(
          eventId: "2", at: "2026-09-30T00:00:01Z", paneId: paneId, kind: .tool_call,
          generation: 1, seq: "4", localId: "child-tool",
          tool: ToolCall(toolCallId: "t", title: "ls", status: .in_progress, locations: []),
          subagentId: "child"))))
  let messages = try #require(client.agentPanes[paneId]?.messages)
  #expect(messages.map(\.kind) == [.subagent, .tool])
  #expect(messages[0].complete == false)
  #expect(messages[1].subagentId == "child")
  #expect(shape(AgentTranscriptGrouping.entries(messages)) == ["Explore[]"])
  guard case .subagent(let group) = AgentTranscriptGrouping.entries(messages).first else {
    Issue.record("expected a subagent row")
    return
  }
  #expect(group.entries.map(\.id) == ["child-tool"])
}

@Test func collapsedPreviewSkipsLeadInsAndFencesForTheMeaningfulContent() {
  #expect(
    AgentSubagentPreview.text("The file exists. Contents verbatim:\n```\nhello\nworld\n```\n")
      == "hello · world")
  #expect(
    AgentSubagentPreview.text("Top-level contents of /srv/app:\n\n- src/\n- README.md\n")
      == "src/ · README.md")
  #expect(
    AgentSubagentPreview.text("## Result\n  The config   is\tin config/app.toml.  ")
      == "Result · The config is in config/app.toml.")
  // Only lead-ins: keep them rather than show nothing.
  #expect(AgentSubagentPreview.text("Found these files:") == "Found these files:")
  #expect(AgentSubagentPreview.text("```\n```\n  \n") == nil)
}

@Test func collapsedPreviewIsCutAtAWordWithAnEllipsis() {
  let long = Array(repeating: "alpha", count: 50).joined(separator: " ")
  let preview = AgentSubagentPreview.text(long, limit: 40)
  #expect(preview == "alpha alpha alpha alpha alpha alpha…")
  #expect((preview?.count ?? 0) <= 40)
  // One unbroken word is cut mid-word.
  #expect(
    AgentSubagentPreview.text(String(repeating: "x", count: 50), limit: 10)
      == String(repeating: "x", count: 9) + "…")
  #expect(AgentSubagentPreview.text("short", limit: 10) == "short")
}

@Test func rowPresentationPreviewsTheSummary() {
  let run = SubagentRun(
    subagentId: "alpha", name: "Explore", task: "Look", status: .completed,
    summary: "Contents verbatim:\n```\nhello\n```")
  let presentation = AgentSubagentPresentation(
    AgentSubagentGroup(subagentId: "alpha", run: run, entries: []))
  #expect(presentation.summary == "hello")
}

@Test func parentWaitsOnlyOnRunningRowsThatEndTheTranscript() {
  func awaited(_ messages: [TranscriptMessage]) -> Int {
    AgentTranscriptGrouping.awaited(AgentTranscriptGrouping.entries(messages))
  }
  #expect(awaited([message(1, "delegating"), row(2, "alpha"), row(3, "beta")]) == 2)
  #expect(
    awaited([
      row(2, "alpha"), row(3, "beta", status: .completed), message(4, "x", owner: "beta"),
    ]) == 1)
  #expect(awaited([row(2, "alpha", status: .completed)]) == 0)
  // Once the parent speaks again it is no longer waiting.
  #expect(awaited([row(2, "alpha"), message(3, "carrying on")]) == 0)
  #expect(AgentSubagentPresentation.waiting(1) == "Waiting for 1 subagent")
  #expect(AgentSubagentPresentation.waiting(2) == "Waiting for 2 subagents")
}
