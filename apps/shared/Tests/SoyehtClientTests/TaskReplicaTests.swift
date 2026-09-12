import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func task(
  id: String = "WS-1", phrase: String = "available to claim",
  state: TaskState = .todo
) -> TaskRecord {
  TaskRecord(
    workspaceId: "workspace", taskId: id, title: "Keep the board current",
    description: "Apply task events", mustNotChange: "The engine owns state", state: state,
    priority: .high, dependencies: [], currentAttempt: 0, prContract: false,
    createdAt: "2026-09-12T00:00:00Z", createdBy: "you", version: 1, phrase: phrase)
}

private func pane() -> Pane {
  Pane(
    paneId: "pane-1", workspaceId: "workspace", name: "Agent", cwd: "/tmp",
    mode: .agent, state: .idle, source: .protocol, confidence: 1, phrase: "Ready",
    generation: 1, cols: 80, rows: 24, record: true)
}

@MainActor @Test func taskSnapshotReplacesBoardAttentionAndLoopFacts() throws {
  let client = EngineClient(socket: "/tmp/sn-task-replica/engine.sock")
  let attention = AttentionItem(
    attentionId: "attention-1", paneId: "pane-1", message: "Needs an answer",
    createdAt: "2026-09-12T00:01:00Z")
  let loop = LoopConfig(
    prompt: "Take the next task", interval: 30, maxRounds: 10, stopWhen: .three_empty,
    enabled: true, rounds: 2, emptyResults: 1)
  try client.applyTaskSnapshot(
    [
      "tasks": [.encoded(task())],
      "attention": [.encoded(attention)],
      "loops": [
        .object(["paneId": .string("pane-1"), "config": try .encoded(loop)])
      ],
    ], panes: [pane()])

  #expect(client.tasks(in: "workspace").map(\.taskId) == ["WS-1"])
  #expect(client.attention(in: "workspace").map(\.attentionId) == ["attention-1"])
  #expect(client.paneLoops["pane-1"]?.rounds == 2)

  try client.applyTaskSnapshot(["tasks": [], "attention": [], "loops": []], panes: [pane()])
  #expect(client.tasks.isEmpty)
  #expect(client.attention(in: "workspace").isEmpty)
  #expect(client.paneLoops.isEmpty)
}

@MainActor @Test func taskEventsUpdateOnlyEnginePublishedFacts() throws {
  let client = EngineClient(socket: "/tmp/sn-task-events/engine.sock")
  let created = task()
  try client.applyTaskEvent(
    .taskCreated,
    .encoded(
      TaskCreatedEvent(
        eventId: "1", at: "2026-09-12T00:00:00Z", task: created)))
  try client.applyTaskEvent(
    .taskPhraseChanged,
    .encoded(
      TaskPhraseChangedEvent(
        eventId: "2", at: "2026-09-12T00:00:01Z", workspaceId: "workspace",
        taskId: "WS-1", phrase: "waiting for your permission")))

  #expect(client.tasks.count == 1)
  #expect(client.tasks.first?.phrase == "waiting for your permission")

  client.taskDetails["WS-1"] = TaskShowResult(
    task: created, subitems: [], comments: [], attempts: [], history: [])
  let comment = Comment(
    commentId: "comment-1", taskId: "WS-1", author: "system", text: "Review returned",
    at: "2026-09-12T00:00:02Z")
  try client.applyTaskEvent(
    .taskCommented,
    .encoded(
      TaskCommentedEvent(
        eventId: "3", at: comment.at, workspaceId: "workspace", taskId: "WS-1",
        comment: comment)))
  #expect(client.taskDetails["WS-1"]?.comments.map(\.commentId) == ["comment-1"])
}

@MainActor @Test func attentionAndLoopEventsReplaceTheirPublishedState() throws {
  let client = EngineClient(socket: "/tmp/sn-task-attention/engine.sock")
  let item = AttentionItem(
    attentionId: "attention-2", paneId: "pane-1", message: "Check failed",
    createdAt: "2026-09-12T00:00:00Z")
  try client.applyAttentionEvent(
    .encoded(
      AttentionChangedEvent(
        eventId: "4", at: "2026-09-12T00:00:03Z", workspaceId: "workspace", count: 1,
        items: [item])))
  #expect(client.attention(in: "workspace").first?.message == "Check failed")

  client.paneLoops["pane-1"] = LoopConfig(
    prompt: "Take the next task", interval: 30, maxRounds: 10, stopWhen: .three_empty,
    enabled: true)
  try client.applyLoopEvent(
    .loopTick,
    .encoded(
      LoopTickEvent(
        eventId: "5", at: "2026-09-12T00:00:04Z", paneId: "pane-1", round: 3,
        of: 10)))
  #expect(client.paneLoops["pane-1"]?.rounds == 3)
  try client.applyLoopEvent(
    .loopStopped,
    .encoded(
      LoopStoppedEvent(
        eventId: "6", at: "2026-09-12T00:00:05Z", paneId: "pane-1",
        reason: "three empty results")))
  #expect(client.paneLoops["pane-1"]?.enabled == false)
}
