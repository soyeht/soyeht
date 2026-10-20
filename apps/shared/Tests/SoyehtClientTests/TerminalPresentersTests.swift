import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The engine's side of a paired device's terminal streams, as the presenters see it.
@MainActor private final class FakeControl: TerminalStreamControl {
  var isConnected = true
  var terminals: Set<String> = ["a", "b"]
  var streams: Set<String> = []
  var alone: Set<String> = []
  var log: [String] = []

  func isTerminal(_ paneId: String) -> Bool { terminals.contains(paneId) }
  func hasStream(_ paneId: String) -> Bool { streams.contains(paneId) }
  func isShownAlone(_ paneId: String) -> Bool { alone.contains(paneId) }
  func subscribeStream(_ paneId: String) async throws {
    log.append("subscribe \(paneId)")
    streams.insert(paneId)
  }
  func unsubscribeStream(_ paneId: String) async throws { log.append("unsubscribe \(paneId)") }
  func dropStream(_ paneId: String) {
    log.append("drop \(paneId)")
    streams.remove(paneId)
  }
  func run(_ action: @escaping @MainActor () async throws -> Void) {
    Task { try? await action() }
  }
}

/// Lets the requests the presenters started finish.
@MainActor private func settle() async {
  for _ in 0..<30 { await Task.yield() }
}

@MainActor private func presenters(
  _ control: FakeControl, _ body: (TerminalPresenters) async throws -> Void
) async rethrows {
  try await body(TerminalPresenters(control: control))
}

@MainActor @Test func aViewMountedWhileConnectedAsksForItsPanesScreen() async {
  let control = FakeControl()
  await presenters(control) { presenters in
    presenters.attach(NSObject(), paneId: "a")
    await settle()
    #expect(control.log == ["subscribe a"])
    #expect(presenters.isPresented("a"))
    #expect(!presenters.isPresented("b"))
  }
}

@MainActor @Test func aViewMountedWithoutAConnectionAsksNothingAndTheConnectionAsksForIt()
  async throws
{
  let control = FakeControl()
  control.isConnected = false
  try await presenters(control) { presenters in
    let first = NSObject()
    let second = NSObject()
    presenters.attach(first, paneId: "a")
    presenters.attach(second, paneId: "b")
    await settle()
    #expect(control.log.isEmpty)

    // The reconnect: the snapshot is in, the views were never rebuilt, no stream exists.
    control.isConnected = true
    try await presenters.resubscribe()
    #expect(control.log == ["subscribe a", "subscribe b"])

    // A pane that already has its stream is not asked twice.
    try await presenters.resubscribe()
    #expect(control.log.count == 2)
  }
}

@MainActor @Test func aViewReplacedBeforeTheOldOneGoesKeepsTheStreamAndGetsTheScreen() async {
  let control = FakeControl()
  await presenters(control) { presenters in
    let old = NSObject()
    let replacement = NSObject()
    presenters.attach(old, paneId: "a")
    await settle()

    // A card rebuilt (rotation, side by side to tab group): its stream exists, its view is new.
    presenters.attach(replacement, paneId: "a")
    await settle()
    #expect(control.log == ["subscribe a", "subscribe a"])

    presenters.detach(old, paneId: "a")
    await settle()
    #expect(control.log == ["subscribe a", "subscribe a"])
    #expect(control.streams == ["a"])
    #expect(presenters.isPresented("a"))

    presenters.detach(replacement, paneId: "a")
    await settle()
    #expect(control.log.suffix(2) == ["drop a", "unsubscribe a"])
    #expect(!presenters.isPresented("a"))
  }
}

@MainActor @Test func theLastViewOfAPaneEndsItsStreamAndNoOtherPane() async {
  let control = FakeControl()
  await presenters(control) { presenters in
    let views = ["a": NSObject(), "b": NSObject()]
    for (paneId, view) in views { presenters.attach(view, paneId: paneId) }
    await settle()

    presenters.detach(views["a"] ?? NSObject(), paneId: "a")
    await settle()

    #expect(control.log.suffix(2) == ["drop a", "unsubscribe a"])
    #expect(control.streams == ["b"])
    #expect(presenters.isPresented("b"))
  }
}

@MainActor @Test func aSinglePaneScreenThatTookThePaneOverKeepsItsStream() async {
  let control = FakeControl()
  await presenters(control) { presenters in
    let board = NSObject()
    presenters.attach(board, paneId: "a")
    await settle()

    // The board dropped to one pane; the single-pane screen subscribed for the same pane.
    control.alone = ["a"]
    presenters.detach(board, paneId: "a")
    await settle()

    #expect(control.log == ["subscribe a"])
    #expect(control.streams == ["a"])
  }
}

@MainActor @Test func aViewThatWasNeverRegisteredEndsNothing() async {
  let control = FakeControl()
  // Both objects stay alive to the end: a freed one would give its identity to the next.
  let registered = NSObject()
  let stranger = NSObject()
  defer { withExtendedLifetime((registered, stranger)) {} }
  await presenters(control) { presenters in
    presenters.attach(registered, paneId: "a")
    await settle()

    presenters.detach(stranger, paneId: "a")
    await settle()

    #expect(control.log == ["subscribe a"])
    #expect(presenters.isPresented("a"))
  }
}

// MARK: The client

/// The requests a client sent to the engine.
@MainActor private final class Requests {
  var log: [String] = []
}

/// A connected client holding terminal panes `a`, `b` and `c`, whose engine answers every request.
@MainActor private func client(selected: String? = nil) throws -> (EngineClient, Requests) {
  let client = EngineClient(socket: "/tmp/terminal-presenters.sock")
  for (index, id) in ["a", "b", "c"].enumerated() {
    try client.applyPaneCreated(
      PaneCreatedEvent(
        eventId: String(index + 1), at: "2026-10-20T12:00:00Z",
        pane: PaneLayoutFixture.pane(id, workspace: "work", mode: .terminal)))
  }
  client.select(selected)
  client.connected = true
  let requests = Requests()
  client.requestStub = { method, params in
    requests.log.append("\(method.rawValue) \(params["paneId"].string ?? "")")
    return .object([:])
  }
  return (client, requests)
}

@MainActor private func installStream(_ client: EngineClient, _ paneId: String) throws {
  try client.terminalStreams.install(
    paneId: paneId,
    info: StreamInfo(
      streamId: "stream-\(paneId)", paneId: paneId, generation: 1, firstRetainedOffset: "0",
      nextOffset: "0", binaryPaneRef: paneId.unicodeScalars.first?.value ?? 1))
}

@MainActor @Test func theSelectionLeavingAPaneKeepsTheStreamWhileAViewShowsIt() async throws {
  let (client, requests) = try client(selected: "a")
  try installStream(client, "a")
  try installStream(client, "b")
  client.terminalPresenters.attach(NSObject(), paneId: "a")
  await settle()
  requests.log.removeAll()

  try await client.unsubscribeAfterSelecting(from: "a")
  try await client.unsubscribeAfterSelecting(from: "b")

  // `a` is on screen in a board; `b` is not, so it ends as before.
  #expect(requests.log == ["pane.stream.unsubscribe b"])
  #expect(client.terminalStreams.contains("a"))
  #expect(!client.terminalStreams.contains("b"))
}

@MainActor @Test func aReconnectAsksForEveryTerminalAViewShowsAndTheSelectedOne() async throws {
  let (client, requests) = try client(selected: "c")
  client.connected = false
  let boardViews = [NSObject(), NSObject()]
  defer { withExtendedLifetime(boardViews) {} }
  client.terminalPresenters.attach(boardViews[0], paneId: "a")
  client.terminalPresenters.attach(boardViews[1], paneId: "b")
  await settle()
  #expect(requests.log.isEmpty)

  // The step a session ends with, as a paired device takes it: the app came back, the snapshot
  // is in, and the board's views never mounted again.
  try await client.settleConnection(followsEveryTerminal: false)

  #expect(client.connected)
  #expect(
    requests.log
      == ["pane.stream.subscribe c", "pane.stream.subscribe a", "pane.stream.subscribe b"])
}

@MainActor @Test func theMacSubscribesEveryTerminalWhenAConnectionSettles() async throws {
  let (client, requests) = try client(selected: "c")
  client.connected = false

  try await client.settleConnection(followsEveryTerminal: true)

  #expect(client.connected)
  #expect(requests.log.sorted() == (["a", "b", "c"].map { "pane.stream.subscribe \($0)" }))
}

@MainActor @Test func aBoardTerminalGoingAwayFreesItsSinkAndStream() async throws {
  let (client, requests) = try client(selected: "b")
  let view = NSObject()
  client.claimTerminalOutput(paneId: "a", owner: view) { _, _ in }
  client.attachBoardTerminal(paneId: "a", owner: view)
  try installStream(client, "a")
  await settle()
  requests.log.removeAll()
  #expect(client.terminalOutputSinkCount == 1)

  client.detachBoardTerminal(paneId: "a", owner: view)
  await settle()

  #expect(client.terminalOutputSinkCount == 0)
  #expect(!client.terminalStreams.contains("a"))
  #expect(requests.log == ["pane.stream.unsubscribe a"])
}

@MainActor @Test func aReplacedBoardTerminalLeavesTheNewViewsSinkAndStream() async throws {
  let (client, requests) = try client(selected: "b")
  var delivered: [String] = []
  let old = NSObject()
  let replacement = NSObject()
  client.claimTerminalOutput(paneId: "a", owner: old) { _, _ in delivered.append("old") }
  client.attachBoardTerminal(paneId: "a", owner: old)
  client.claimTerminalOutput(paneId: "a", owner: replacement) { _, _ in delivered.append("new") }
  client.attachBoardTerminal(paneId: "a", owner: replacement)
  try installStream(client, "a")
  await settle()
  requests.log.removeAll()

  client.detachBoardTerminal(paneId: "a", owner: old)
  await settle()
  client.deliverTerminalOutput(to: "a", bytes: Data("x".utf8), reset: false)

  #expect(requests.log.isEmpty)
  #expect(delivered == ["new"])
  #expect(client.terminalStreams.contains("a"))
}

@MainActor @Test func theBoardGivingWayToTheSinglePaneScreenDoesNotEndItsStream() async throws {
  let (client, requests) = try client(selected: "a")
  let boardView = NSObject()
  client.claimTerminalOutput(paneId: "a", owner: boardView) { _, _ in }
  client.attachBoardTerminal(paneId: "a", owner: boardView)
  await settle()

  // The single-pane screen follows the selection on the one sink and subscribes by itself.
  client.claimTerminalOutput(owner: NSObject()) { _, _ in }
  try installStream(client, "a")
  requests.log.removeAll()
  client.detachBoardTerminal(paneId: "a", owner: boardView)
  await settle()

  #expect(requests.log.isEmpty)
  #expect(client.terminalStreams.contains("a"))
}
