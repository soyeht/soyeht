import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The engine queues the pane's tail (`ProtocolLimits.tailBytes`) for a connection on every
/// subscribe from the tail, and closes a connection whose queue passes
/// `ProtocolLimits.connectionQueueBytes`. The model below holds that queue: a subscribe adds a
/// tail, and the tail leaves it when the client has read the replay's end, as the engine's writer
/// frees what it has put on the socket. It models neither live output nor a replay that never
/// ends: it says that the client sends no subscribe before the previous replay has ended, which
/// is what keeps the tails from adding up. The real engine is the end-to-end acceptance's.

@MainActor private final class EngineQueueModel {
  private(set) var replaysQueued = 0
  private(set) var peakReplays = 0
  var peakBytes: Int { peakReplays * ProtocolLimits.tailBytes }
  private(set) var subscribed: [String] = []
  private var references: [String: UInt32] = [:]
  unowned let client: EngineClient

  init(_ client: EngineClient) { self.client = client }

  /// The engine's answer to a subscribe: the stream, then the replay, which ends a moment later.
  func subscribe(_ paneId: String) throws -> JsonValue {
    subscribed.append(paneId)
    replaysQueued += 1
    peakReplays = max(peakReplays, replaysQueued)
    let streamId = "stream-\(paneId)"
    let reference = references[paneId] ?? UInt32(references.count + 1)
    references[paneId] = reference
    try client.terminalStreams.install(
      paneId: paneId,
      info: StreamInfo(
        streamId: streamId, paneId: paneId, generation: 1, firstRetainedOffset: "0",
        nextOffset: "0", binaryPaneRef: reference))
    try client.receive(
      message: notification(
        .streamReplayStart,
        StreamReplayStartEvent(
          streamId: streamId, generation: 1, fromOffset: "0", toOffset: "0", reset: true)))
    Task { @MainActor in
      for _ in 0..<5 { await Task.yield() }
      self.replaysQueued -= 1
      try? self.client.receive(
        message: self.notification(
          .streamReplayDone,
          StreamReplayDoneEvent(
            streamId: streamId, nextOffset: "0")))
    }
    return .object([:])
  }

  private func notification<T: Encodable>(_ event: EventName, _ params: T) throws -> JsonValue {
    .object([
      "jsonrpc": .string("2.0"), "method": .string(event.rawValue), "params": try .encoded(params),
    ])
  }
}

/// `count` terminals with a surface each: the first third in the workspace on screen, the rest in
/// another one. The first is selected.
@MainActor private func terminals(_ count: Int) throws -> (EngineClient, EngineQueueModel) {
  let client = EngineClient(socket: "/tmp/terminal-replay-budget.sock")
  for index in 0..<count {
    try client.applyPaneCreated(
      PaneCreatedEvent(
        eventId: String(index + 1), at: "2026-11-18T12:00:00Z",
        pane: PaneLayoutFixture.pane(
          "t\(index)", workspace: index < (count + 2) / 3 ? "on-screen" : "other",
          mode: .terminal)))
    client.claimTerminalOutput(paneId: "t\(index)", owner: client) { _, _ in }
  }
  client.select("t0")
  let model = EngineQueueModel(client)
  client.requestStub = { method, params in
    guard method == .paneStreamSubscribe, let paneId = params["paneId"].string else {
      return .object(["items": .array([])])
    }
    return try model.subscribe(paneId)
  }
  return (client, model)
}

@MainActor private func finish(_ model: EngineQueueModel, _ count: Int) async {
  for _ in 0..<2_000 where model.subscribed.count < count || model.replaysQueued > 0 {
    await Task.yield()
  }
}

@MainActor @Test func theTailsOfManyTerminalsAreNeverQueuedTogether() async throws {
  let count = 12
  let (client, model) = try terminals(count)
  // The views of a grid mount while the connection settles and each asks for its screen.
  client.connected = true
  for index in 0..<count { client.refreshTerminal("t\(index)") }

  try await client.settleConnection(followsEveryTerminal: true)
  await finish(model, count)

  #expect(model.subscribed.sorted() == (0..<count).map { "t\($0)" }.sorted())
  #expect(model.peakReplays == 1)
  #expect(model.peakBytes == ProtocolLimits.tailBytes)
  #expect(model.peakBytes < ProtocolLimits.connectionQueueBytes)

  // The connection dropped and the session settles again: the same order, again one at a time.
  let first = model.subscribed
  client.stop()
  try await client.settleConnection(followsEveryTerminal: true)
  await finish(model, 2 * count)
  #expect(Array(model.subscribed.dropFirst(count)) == first)
  #expect(model.peakReplays == 1)
}

@MainActor @Test func aTerminalFirstShownAfterAReconnectGetsItsHistory() async throws {
  let client = EngineClient(socket: "/tmp/terminal-replay-budget.sock")
  for (id, workspace) in [("first", "w1"), ("later", "w2")] {
    try client.applyPaneCreated(
      PaneCreatedEvent(
        eventId: id, at: "2026-11-18T12:00:00Z",
        pane: PaneLayoutFixture.pane(id, workspace: workspace, mode: .terminal)))
  }
  client.select("first")
  client.claimTerminalOutput(paneId: "first", owner: client) { _, _ in }
  let model = EngineQueueModel(client)
  client.requestStub = { _, params in try model.subscribe(params["paneId"].string ?? "") }

  // The app was reopened: the workspace on screen has its surface, the other has none yet.
  try await client.settleConnection(followsEveryTerminal: true)
  await finish(model, 1)
  #expect(model.subscribed == ["first"])

  // The person opens the other workspace: its surface appears and asks for the screen.
  client.claimTerminalOutput(paneId: "later", owner: client) { _, _ in }
  client.refreshTerminal("later")
  await finish(model, 2)
  #expect(model.subscribed == ["first", "later"])
}

@MainActor @Test func aHolderOfAnEndedConnectionNeverTakesTheNewHoldersWait() async {
  let gate = TerminalReplayGate()
  let old = await gate.enter()
  gate.reset()
  let new = await gate.enter()
  var newEnded = false
  let waiting = Task { @MainActor in
    await gate.awaitReplayEnd(of: "b", turn: new ?? -1)
    newEnded = true
  }
  await Task.yield()

  // The old connection's subscriber comes back late: it neither waits nor disturbs the new wait.
  await gate.awaitReplayEnd(of: "a", turn: old ?? -1)
  gate.leave(old ?? -1)
  #expect(!newEnded)
  gate.replayEnded("b")
  await waiting.value
  #expect(newEnded)
}
