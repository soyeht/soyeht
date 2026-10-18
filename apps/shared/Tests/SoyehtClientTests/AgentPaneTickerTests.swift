import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private let start = Date(timeIntervalSinceReferenceDate: 800_000_000)

private func pane(state: PaneState, title: String? = nil, activePromptId: String? = "p1") -> Pane {
  Pane(
    paneId: "pane", workspaceId: "w", name: "Codex", cwd: "/", mode: .agent, state: state,
    source: .protocol, confidence: 1, phrase: "", generation: 1, cols: 80, rows: 24,
    record: false,
    agent: AgentSession(
      harness: "registry.codex-acp", version: "1", fingerprint: "f", options: [],
      authMethods: [], queue: [], draft: false, activePromptId: activePromptId, role: .worker),
    title: title)
}

private func tool(_ id: String, _ title: String, _ status: ToolStatus) -> TranscriptMessage {
  PaneInteriorFixture.tool(id, title: title, status: status)
}

private func plan(_ entries: [(String, PlanStatus)]) -> TranscriptMessage {
  TranscriptMessage(
    localId: "plan", generation: 1, seq: "0", role: .assistant, kind: .plan, text: "",
    complete: true,
    plan: entries.map { PlanEntry(content: $0.0, priority: .medium, status: $0.1) })
}

@Test func acpTickerLeadsWithThePlanStepAndTheRunningToolThenEarlierTools() {
  var view = AgentPaneView()
  view.messages = [
    plan([
      ("Read the schema", .completed), ("Run the contract tests", .in_progress),
      ("Report", .pending),
    ]),
    tool("1", "schema/v2/meta.json", .completed),
    tool("2", "cargo fmt", .failed),
    tool("3", "cargo test -p acp", .in_progress),
  ]
  let snapshot = AgentACPTickerSource().snapshot(
    for: AgentTickerContext(
      pane: pane(state: .working, title: "ACP v2 client"), view: view, turnStartedAt: start,
      now: start.addingTimeInterval(134)))

  #expect(
    snapshot.items.map(\.text) == [
      "Step 2/3: Run the contract tests · Running cargo test -p acp",
      "Failed: cargo fmt",
      "Done: schema/v2/meta.json",
      "ACP v2 client",
    ])
  #expect(snapshot.elapsed == "2m 14s")
}

private func prompt(_ seq: String) -> TranscriptMessage {
  TranscriptMessage(
    localId: "prompt-\(seq)", generation: 1, seq: seq, role: .user, kind: .message, text: "go",
    complete: true)
}

private func ticker(_ messages: [TranscriptMessage]) -> [String] {
  var view = AgentPaneView()
  view.messages = messages
  return AgentACPTickerSource().snapshot(
    for: AgentTickerContext(
      pane: pane(state: .idle, activePromptId: nil), view: view, turnStartedAt: nil, now: start)
  ).items.map(\.text)
}

@Test func aFailedToolCallLeavesTheTickerWhenTheNextTurnStarts() {
  let failed = [prompt("1"), tool("2", "Write qa-out.txt", .failed)]
  #expect(ticker(failed) == ["Failed: Write qa-out.txt"])
  #expect(ticker(failed + [prompt("3")]).isEmpty)
  #expect(
    ticker(failed + [prompt("3"), tool("4", "Read notes", .completed)]) == ["Done: Read notes"])
}

@Test func aFailedToolCallLeavesTheTickerOnceALaterCallOfTheTurnSucceeds() {
  let messages = [
    prompt("1"), tool("2", "Write qa-out.txt", .failed), tool("3", "Write qa-out.txt", .completed),
  ]
  #expect(ticker(messages) == ["Done: Write qa-out.txt"])
}

@Test func anEarlierTurnsPlanDoesNotLeadTheTicker() {
  let messages = [
    prompt("1"), plan([("Old step", .in_progress)]), prompt("3"),
  ]
  #expect(ticker(messages).isEmpty)
}

@Test func acpTickerShowsNoElapsedTimeOnceTheTurnEnds() {
  var view = AgentPaneView()
  view.messages = [tool("1", "cargo test", .completed)]
  let snapshot = AgentACPTickerSource().snapshot(
    for: AgentTickerContext(
      pane: pane(state: .idle), view: view, turnStartedAt: start, now: start + 60))
  #expect(snapshot.items.map(\.text) == ["Done: cargo test"])
  #expect(snapshot.elapsed == nil)
}

@Test func acpTickerIsEmptyBeforeAnythingHappens() {
  let snapshot = AgentACPTickerSource().snapshot(
    for: AgentTickerContext(
      pane: pane(state: .idle, activePromptId: nil), view: AgentPaneView(), turnStartedAt: nil,
      now: start))
  #expect(snapshot == AgentTickerSnapshot(items: [], elapsed: nil))
}

@Test func toolHistoryIsCapped() {
  var view = AgentPaneView()
  view.messages = (1...8).map { tool("\($0)", "step \($0)", .completed) }
  let items = AgentACPTickerSource().snapshot(
    for: AgentTickerContext(pane: pane(state: .idle), view: view, turnStartedAt: nil, now: start)
  ).items
  #expect(items.count == 1 + AgentACPTickerSource.toolHistory)
  #expect(items.first?.text == "Done: step 8")
}

/// A summarizer plugs in through the protocol; the header only reads snapshots.
private struct RecapSource: AgentTickerSource {
  func snapshot(for context: AgentTickerContext) -> AgentTickerSnapshot {
    AgentTickerSnapshot(
      items: [AgentTickerItem(id: "recap", text: "Waiting for your OK", time: "09:40")],
      elapsed: nil)
  }
}

@Test func anotherSourceReplacesTheACPTickerWithoutTheHeaderChanging() {
  let source: any AgentTickerSource = RecapSource()
  let snapshot = source.snapshot(
    for: AgentTickerContext(pane: nil, view: AgentPaneView(), turnStartedAt: nil, now: start))
  #expect(snapshot.items.first?.time == "09:40")
}

@Test func turnClockStartsWhenAPromptBecomesActiveAndStopsWhenItEnds() {
  var clock = AgentTurnClock()
  clock.observe(activePromptId: "p1", now: start)
  clock.observe(activePromptId: "p1", now: start + 30)
  #expect(clock.startedAt == start)
  clock.observe(activePromptId: "p2", now: start + 60)
  #expect(clock.startedAt == start + 60)
  clock.observe(activePromptId: nil, now: start + 90)
  #expect(clock.startedAt == nil)
}

@Test func headerAsksForThePersonOnPermissionsAndWaitingStates() {
  #expect(AgentHeaderStatus.needsYou(state: .waiting_for_you, view: AgentPaneView()))
  #expect(!AgentHeaderStatus.needsYou(state: .working, view: AgentPaneView()))
}
