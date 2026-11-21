import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The window chrome is what shows no pane's text: the tab strip, the rail, the root of the window
/// and the grid of the workspace on screen. An agent that works in a pane no view shows must run
/// none of it unless what the chrome shows of that pane changed. Each reader below is what a view
/// body of the Mac app reads; the tests count how often it is told, which says the same on any
/// machine.

/// A window on workspace `w1`, which shows the agent pane `v` and holds the agent pane `t` in
/// another tab of its group, and workspace `w2`, whose agent pane `h` nobody shows.
@MainActor private struct Window {
  let client = EngineClient(socket: "/tmp/sn-hidden-chrome/engine.sock")

  init() {
    client.connected = true
    client.panes = [
      agentPane("v", workspaceId: "w1"), agentPane("t", workspaceId: "w1"),
      agentPane("h", workspaceId: "w2"),
    ]
    for id in ["v", "t", "h"] { client.agentPanes[id, default: AgentPaneView()].merge([]) }
    for id in ["w1", "w2"] {
      client.upsertWorkspace(Workspace(workspaceId: id, name: id, path: "/tmp", layout: .null))
    }
    client.select("v")
    showPane(client, "v")
  }

  /// What each view of the chrome reads, by name.
  var readers: [(String, Changes)] {
    let client = client
    return [
      (
        "the root of the window",
        Changes {
          _ = client.agentAttentionRequests
          _ = client.paneOutline(of: client.selectedPaneId)
          _ = client.workspaces
          _ = client.connected
        }
      ),
      ("the tab of w1", Changes { _ = client.agentAttention(in: "w1") }),
      ("the tab of w2", Changes { _ = client.agentAttention(in: "w2") }),
      ("the rail", Changes { _ = (client.agentWaitingTotal, client.agentWorking) }),
      ("the usage rings", Changes { _ = client.usage.readings }),
      ("the grid of w1", Changes { _ = client.outlinedPanes(in: "w1") }),
      ("the card of v", Changes { _ = (client.pane("v"), client.agentPanes.isAsking("v")) }),
    ]
  }

  func pane(_ id: String, queue: Int, active: Bool, state: PaneState) throws -> JsonValue {
    var pane = agentPane(id, workspaceId: id == "h" ? "w2" : "w1", state: state)
    pane.agent = AgentSession(
      harness: "test", version: "1", fingerprint: "f", options: [], authMethods: [],
      queue: (0..<queue).map {
        QueuedPromptSummary(
          promptId: "p\($0)", origin: .human, status: .queued, queuedAt: "t", generation: 1)
      }, draft: false, activePromptId: active ? "p0" : nil, role: .worker)
    pane.phrase = active ? "Agent is working" : "Agent turn ended"
    return .object(["pane": try .encoded(pane)])
  }

  /// One turn of the agent `id`, in the order the engine sends it: the pane with its queue, the
  /// pane working, its state, the growing text, the usage line and the finished reply, the pane
  /// idle, its state, the end of the turn.
  func turn(_ id: String) throws {
    try client.applyPaneEvent(.paneUpdated, pane(id, queue: 1, active: false, state: .idle))
    try client.applyPaneEvent(.paneUpdated, pane(id, queue: 0, active: true, state: .working))
    try client.applyPaneEvent(.paneStateChanged, stateChanged(id, .working, "Agent is working"))
    for round in 1...50 { try client.applyAgentEvent(.paneUpdate, partial(id, "reply \(round)")) }
    try client.applyAgentEvent(.paneUpdate, finished(id))
    try client.applyPaneEvent(.paneUpdated, pane(id, queue: 0, active: false, state: .idle))
    try client.applyPaneEvent(.paneStateChanged, stateChanged(id, .idle, "Agent turn ended"))
    try client.applyAgentEvent(
      .paneTurnEnded,
      .encoded(
        PaneTurnEndedEvent(
          eventId: "9", at: "t", paneId: id, promptId: "p0", stopReason: .end_turn)))
  }
}

private func stateChanged(_ paneId: String, _ state: PaneState, _ phrase: String) throws
  -> JsonValue
{
  try .encoded(
    PaneStateChangedEvent(
      eventId: "2", at: "t", paneId: paneId, state: state, confidence: 1, phrase: phrase,
      source: .protocol))
}

private func partial(_ paneId: String, _ text: String) throws -> JsonValue {
  try .encoded(
    PaneUpdateEvent.v0(
      AgentMessageUpdate(
        eventId: "1", at: "t", paneId: paneId, kind: .agent_message_chunk, generation: 1,
        seq: "900", localId: "reply", text: text, complete: false)))
}

private func finished(_ paneId: String) throws -> JsonValue {
  try .encoded(
    PaneUpdateEvent.v0(
      AgentMessageUpdate(
        eventId: "1", at: "t", paneId: paneId, kind: .agent_message_chunk, generation: 1,
        seq: "900", localId: "reply", text: "the reply", complete: true)))
}

@MainActor @Test func aTurnOfAnAgentNobodyShowsRunsOnlyTheTabOfItsWorkspace() throws {
  let window = Window()
  let told = window.readers
  for _ in 1...5 { try window.turn("h") }
  let counts = Dictionary(uniqueKeysWithValues: told.map { ($0.0, $0.1.count) })
  // The tab of w2 shows whether an agent works there: it starts and stops with each turn.
  #expect(counts["the tab of w2"] == 10)
  // The rail shows whether any agent works: the same two changes.
  #expect(counts["the rail"] == 10)
  for (name, count) in counts where !["the tab of w2", "the rail"].contains(name) {
    #expect(count == 0, "a turn of an agent in w2 ran \(name) \(count) times")
  }
}

@MainActor @Test func anAgentInAnotherTabOfTheWorkspaceOnScreenRunsNoReaderOfTheOthers() throws {
  let window = Window()
  let told = window.readers
  for _ in 1...5 { try window.turn("t") }
  let counts = Dictionary(uniqueKeysWithValues: told.map { ($0.0, $0.1.count) })
  #expect(counts["the tab of w1"] == 10 && counts["the rail"] == 10)
  for (name, count) in counts where !["the tab of w1", "the rail"].contains(name) {
    #expect(count == 0, "a turn of the agent t ran \(name) \(count) times")
  }
}

@MainActor @Test func aTurnOfTheShownPaneRunsItsCardAndNotTheRootOfTheWindow() throws {
  let window = Window()
  let told = window.readers
  for _ in 1...5 { try window.turn("v") }
  let counts = Dictionary(uniqueKeysWithValues: told.map { ($0.0, $0.1.count) })
  #expect(counts["the card of v"] ?? 0 > 0, "a pane on screen keeps its live updates")
  #expect(counts["the root of the window"] == 0 && counts["the usage rings"] == 0)
  #expect(counts["the grid of w1"] == 0 && counts["the tab of w2"] == 0)
}

@MainActor @Test func aStateTheEngineSendsAgainIsNotAChange() throws {
  let window = Window()
  try window.client.applyPaneEvent(
    .paneStateChanged, stateChanged("h", .working, "Agent is working"))
  let told = Changes { _ = window.client.pane("h") }
  let slice = Changes { _ = window.client.panes(in: "w2") }
  for _ in 1...20 {
    try window.client.applyPaneEvent(
      .paneStateChanged, stateChanged("h", .working, "Agent is working"))
  }
  #expect(told.count == 0 && slice.count == 0)
}

@MainActor @Test func aNewRequestInAHiddenPaneStillReachesTheWindow() throws {
  let window = Window()
  let told = window.readers
  try window.client.applyAgentEvent(
    .panePermissionRequested,
    .encoded(
      PanePermissionRequestedEvent(
        eventId: "3", at: "t", paneId: "h", requestId: "r", generation: 1, promptId: "p",
        title: "Write", detail: "", presentation: nil, options: [], expiresAt: "later")))
  let counts = Dictionary(uniqueKeysWithValues: told.map { ($0.0, $0.1.count) })
  #expect(counts["the root of the window"] == 1, "the notification and the dock badge")
  #expect(counts["the tab of w2"] == 1 && counts["the rail"] == 1, "the badges")
  #expect(counts["the tab of w1"] == 0 && counts["the card of v"] == 0)
  #expect(window.client.agentAttention(in: "w2").waiting == 1)
  #expect(window.client.agentWaitingTotal == 1)
}

@MainActor @Test func theGridFollowsAPaneThatComesGoesOrIsRenamed() throws {
  let window = Window()
  let grid = Changes { _ = window.client.outlinedPanes(in: "w1") }
  var renamed = agentPane("t", workspaceId: "w1")
  renamed.name = "renamed"
  try window.client.applyPaneEvent(.paneRenamed, .object(["pane": .encoded(renamed)]))
  try window.client.applyPaneEvent(.paneClosed, .object(["paneId": .string("t")]))
  #expect(grid.count == 2)
  #expect(window.client.outlinedPanes(in: "w1").map(\.paneId) == ["v"])
}
