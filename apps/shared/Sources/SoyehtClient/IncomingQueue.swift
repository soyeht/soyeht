import Foundation
import SoyehtProtocol
import os

/// What the reader hands the main actor, in the order it was read.
enum Incoming: Sendable {
  /// A notification, or a request the engine makes of the app, parsed.
  case message(JsonValue)
  /// The answer to a request. The request is already out of `PendingRequests` (its deadline is
  /// cancelled); the main actor applies the answer's state and resumes the caller.
  case reply(PendingRequests.Entry, JsonValue)
  /// A terminal or attachment frame.
  case other(Frame)
  /// The connection ended: nothing follows.
  case failure(Error)
}

/// The ordered hand-off from the reader, which runs on no actor, to the main actor, which applies
/// what was read. The reader never waits for the main actor: a main thread that is busy for
/// seconds lets this fill, and the reader keeps matching answers to requests and keeping the
/// engine's output moving (ADR 0173). The main actor takes everything that has arrived in one go
/// and applies it in order.
///
/// What waits here is bounded by `ClientPolicy.capsIncomingQueueBytes`: a main actor that falls
/// further behind than that has lost the screen anyway, so the connection ends and the reconnect
/// hydrates the replicas again instead of the app growing without a limit.
final class IncomingQueue: Sendable {
  private struct State {
    var items: [Incoming] = []
    var bytes = 0
    var waiter: CheckedContinuation<[Incoming], Never>?
    var finished = false
    var failure: Error?
  }

  private let state = OSAllocatedUnfairLock(initialState: State())
  private let limitBytes: Int

  init(limitBytes: Int = ClientPolicy.capsIncomingQueueBytes) { self.limitBytes = limitBytes }

  /// Queues `item`, which weighs `bytes`. `false` when the queue is full: nothing was queued and
  /// the caller ends the connection.
  func push(_ item: Incoming, bytes: Int = 0) -> Bool {
    enum Outcome {
      case queued(Bool)
      case finished
      case wake(CheckedContinuation<[Incoming], Never>, [Incoming])
    }
    let outcome = state.withLock { state -> Outcome in
      guard !state.finished else { return .finished }
      state.items.append(item)
      state.bytes += bytes
      guard let waiter = state.waiter else { return .queued(state.bytes <= limitBytes) }
      state.waiter = nil
      defer { (state.items, state.bytes) = ([], 0) }
      return .wake(waiter, state.items)
    }
    switch outcome {
    case .queued(let fits): return fits
    case .wake(let waiter, let items):
      waiter.resume(returning: items)
      return true
    case .finished:
      // Nothing applies it any more: a request waiting for this answer fails with the rest.
      if case .reply(let request, _) = item {
        request.continuation.resume(throwing: ClientFailure.disconnected)
      }
      return true
    }
  }

  /// Everything that has arrived and not been taken, waiting for the first item when there is
  /// none. Empty once the queue is finished.
  func next() async -> [Incoming] {
    await withCheckedContinuation { continuation in
      let ready = state.withLock { state -> [Incoming]? in
        if !state.items.isEmpty {
          defer { (state.items, state.bytes) = ([], 0) }
          return state.items
        }
        if state.finished { return [] }
        state.waiter = continuation
        return nil
      }
      if let ready { continuation.resume(returning: ready) }
    }
  }

  /// Ends the queue. What was never applied fails: a request whose answer was read but not
  /// applied is resumed with `error`, so no caller waits for ever, and the consumer stops.
  func finish(failing error: Error? = nil) {
    let reason = error ?? ClientFailure.disconnected
    let (waiter, dropped) = state.withLock { state in
      state.finished = true
      state.failure = error
      defer { (state.items, state.bytes, state.waiter) = ([], 0, nil) }
      return (state.waiter, state.items)
    }
    for case .reply(let request, _) in dropped { request.continuation.resume(throwing: reason) }
    waiter?.resume(returning: [])
  }

  /// Why the queue ended, when it ended because something was wrong (a full backlog).
  var failure: Error? { state.withLock { $0.failure } }

  var queuedBytes: Int { state.withLock { $0.bytes } }
}
