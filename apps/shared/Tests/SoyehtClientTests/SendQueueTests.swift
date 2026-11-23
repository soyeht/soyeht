import Foundation
import SoyehtProtocol
import Testing
import os

@testable import SoyehtClient

/// A transport that takes a unit one message at a time, with a suspension after each, the way a
/// WebSocket does: anything that can interleave does.
@MainActor
private final class FakeTransport {
  private(set) var sent: [String] = []
  private let failureLog = OSAllocatedUnfairLock<[Error]>(initialState: [])
  var failures: [Error] { failureLog.withLock { $0 } }
  var failOn: String?
  var holdAfter: String?
  private var release: CheckedContinuation<Void, Never>?
  private(set) var holding = false

  func transmit(_ unit: [String]) async throws {
    for message in unit {
      if failOn == message { throw ClientFailure.disconnected }
      sent.append(message)
      if holdAfter == message {
        holdAfter = nil
        holding = true
        await withCheckedContinuation { release = $0 }
        holding = false
      }
      await Task.yield()
    }
  }

  func resume() {
    release?.resume()
    release = nil
  }

  func queue() -> SendQueue<[String]> {
    let failureLog = failureLog
    return SendQueue(
      transmit: { [unowned self] unit in try await self.transmit(unit) },
      onFailure: { error in failureLog.withLock { $0.append(error) } })
  }
}

/// An upload held after its first frame, and a unit of its own queued behind it.
@MainActor
private func heldUpload(
  _ transport: FakeTransport, _ queue: SendQueue<[String]>, behind: String
) async -> (upload: Task<Void, Error>, behind: Task<Void, Error>) {
  transport.holdAfter = "photo"
  let upload = Task { try await queue.send(["photo", "prompt"]) }
  while !transport.holding { await Task.yield() }
  let waiting = Task { try await queue.send([behind]) }
  await settle()
  return (upload, waiting)
}

private final class Failures: Sendable {
  private let lock = OSAllocatedUnfairLock<[Error?]>(initialState: [])
  var errors: [Error?] { lock.withLock { $0 } }
  func add(_ error: Error?) { lock.withLock { $0.append(error) } }
}

private final class Completions: Sendable {
  private let lock = OSAllocatedUnfairLock(initialState: 0)
  var count: Int { lock.withLock { $0 } }
  func add() { lock.withLock { $0 += 1 } }
}

@MainActor private func settle() async {
  for _ in 0..<20 { await Task.yield() }
}

@MainActor
@Test func aUnitWithAttachmentsLeavesWholeWhileOtherRequestsAreSentMeanwhile() async throws {
  let transport = FakeTransport()
  let queue = transport.queue()
  let units: [[String]] = [
    ["photo-1", "photo-2", "prompt"], ["state"], ["list"], ["a", "b", "c", "commit"], ["ping"],
  ]
  let left = Completions()
  for unit in units { queue.enqueue(unit) { _ in left.add() } }
  while left.count < units.count { await Task.yield() }
  #expect(transport.sent == units.flatMap { $0 })
}

@MainActor
@Test func requestsMadeConcurrentlyKeepTheOrderTheyWereQueuedIn() async throws {
  let transport = FakeTransport()
  let queue = transport.queue()
  let names = (0..<30).map { "request-\($0)" }
  let left = Completions()
  for name in names { queue.enqueue([name]) { _ in left.add() } }
  while left.count < names.count { await Task.yield() }
  #expect(transport.sent == names)
}

@MainActor
@Test func aWaitingUnitThatIsCancelledSendsNothing() async throws {
  let transport = FakeTransport()
  transport.holdAfter = "first-frame"
  let queue = transport.queue()
  let first = Task { try await queue.send(["first-frame", "first-request"]) }
  while !transport.holding { await Task.yield() }
  let cancelled = Task { try await queue.send(["photo", "cancelled-request"]) }
  let after = Task { try await queue.send(["after"]) }
  await settle()
  cancelled.cancel()
  await #expect(throws: CancellationError.self) { try await cancelled.value }
  transport.resume()
  try await first.value
  try await after.value
  #expect(transport.sent == ["first-frame", "first-request", "after"])
}

@MainActor
@Test func aUnitAlreadyLeavingFinishesWhenItsCallerIsCancelled() async throws {
  let transport = FakeTransport()
  let queue = transport.queue()
  let (upload, other) = await heldUpload(transport, queue, behind: "other")
  upload.cancel()
  await settle()
  #expect(transport.sent == ["photo"])
  transport.resume()
  _ = await upload.result  // Whether the caller sees its own cancellation is not the point.
  try await other.value
  // The frames were not left on their own: the request that commits them followed.
  #expect(transport.sent == ["photo", "prompt", "other"])
}

@MainActor
@Test func aCallerCancelledBeforeItQueuesSendsNothing() async throws {
  let transport = FakeTransport()
  let queue = transport.queue()
  let task = Task {
    while !Task.isCancelled { await Task.yield() }
    try await queue.send(["never"])
  }
  task.cancel()
  await #expect(throws: CancellationError.self) { try await task.value }
  #expect(transport.sent.isEmpty)
}

@MainActor
@Test func aFailureInsideAUnitEndsTheConnectionAndFailsTheUnitsBehindIt() async throws {
  let transport = FakeTransport()
  transport.failOn = "prompt"
  let queue = transport.queue()
  let ended = Failures()
  queue.enqueue(["photo", "prompt"]) { ended.add($0) }
  queue.enqueue(["behind"]) { ended.add($0) }
  while ended.errors.count < 2 { await Task.yield() }
  #expect(ended.errors.allSatisfy { $0 is ClientFailure })
  await #expect(throws: ClientFailure.self) { try await queue.send(["later"]) }
  #expect(transport.sent == ["photo"])
  #expect(transport.failures.count == 1)
}

@MainActor
@Test func failingTheQueueFailsTheUnitsWaitingAndLaterOnes() async throws {
  let transport = FakeTransport()
  let queue = transport.queue()
  let (upload, waiting) = await heldUpload(transport, queue, behind: "waiting")
  queue.fail(ClientFailure.timeout)
  await #expect(throws: ClientFailure.self) { try await waiting.value }
  await #expect(throws: ClientFailure.self) { try await queue.send(["later"]) }
  transport.resume()
  try await upload.value
  #expect(transport.sent == ["photo", "prompt"])
}

@Test func theLocalSocketTakesEveryFrameOfAUnitInOneWrite() throws {
  let frames = try WirePayload(
    message: .object(["method": .string("pane.prompt")]), attachments: [attachment(bytes: 3)]
  ).frames()
  #expect(frames.count == 2)
  let unit = try Outgoing.unit(frames, singleWrite: true)
  var expected = Data()
  for frame in frames { expected.append(try frame.encode()) }
  #expect(unit == [.binary(expected)])
}

@Test func aWebSocketTakesTheJsonFrameAsTextAndEachOtherFrameAsBinary() throws {
  let frames = try WirePayload(
    message: .object(["method": .string("pane.prompt")]), attachments: [attachment(bytes: 3)]
  ).frames()
  let unit = try Outgoing.unit(frames, singleWrite: false)
  #expect(unit.count == 2)
  #expect(unit.first == .binary(try frames[0].encode()))
  #expect(unit.last == .text(String(decoding: frames[1].payload, as: UTF8.self)))
}

private func attachment(bytes: Int) -> WebHostAttachment {
  WebHostAttachment(
    reference: AttachmentRef(
      attachmentId: UUID().uuidString.lowercased(), mimeType: "image/png", name: "photo.png",
      bytes: String(bytes)),
    bytes: Data(repeating: 7, count: bytes))
}
