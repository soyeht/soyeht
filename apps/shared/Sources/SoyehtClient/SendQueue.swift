import Foundation
import os

/// One serial queue for everything a connection sends. A request that carries binary attachments
/// leaves as several frames followed by the request, and the engine commits those frames to "the
/// following request" (`AttachmentAssembler`), so the whole unit must leave with nothing between
/// its frames. The queue sends one unit at a time, in the order the units were queued, whatever the
/// transport: a WebSocket takes one message per frame and each takes a suspension, a local socket
/// takes one write.
///
/// Cancellation: a unit still waiting is removed and nothing of it is sent. A unit already
/// leaving runs to its end, because stopping after the frames and before the request would leave
/// them pending on the engine, which then refuses every request until one commits them. A
/// transport failure inside a unit ends the connection for the same reason: the units behind it
/// fail with that error and `onFailure` closes the wire, so the engine drops the half unit with the
/// connection.
///
/// Units are queued synchronously, in the order of the calls, from any actor, and sent by a task of
/// no actor: nothing a connection sends waits for the main actor, and two requests made one after
/// the other reach the engine in that order (ADR 0171).
final class SendQueue<Unit: Sendable>: Sendable {
  private struct Entry {
    let id: UInt64
    let unit: Unit
    let completion: @Sendable (Error?) -> Void
  }

  private struct State {
    var waiting: [Entry] = []
    var draining = false
    var failure: Error?
    var nextId: UInt64 = 0
  }

  private enum Action {
    case none, drain
    case refuse(Error)
  }

  private let state = OSAllocatedUnfairLock(initialState: State())
  private let transmit: @Sendable (Unit) async throws -> Void
  private let onFailure: @Sendable (Error) -> Void

  /// `transmit` puts one whole unit on the transport; `onFailure` runs once, when it throws.
  init(
    transmit: @escaping @Sendable (Unit) async throws -> Void,
    onFailure: @escaping @Sendable (Error) -> Void
  ) {
    self.transmit = transmit
    self.onFailure = onFailure
  }

  /// Queues `unit` behind the ones queued before it; `completion` runs when it has left (`nil`) or
  /// why it did not. Returns the unit's id, for `cancel`.
  @discardableResult
  func enqueue(_ unit: Unit, completion: @escaping @Sendable (Error?) -> Void) -> UInt64 {
    let (id, action) = state.withLock { state -> (UInt64, Action) in
      if let failure = state.failure { return (0, .refuse(failure)) }
      state.nextId &+= 1
      let id = state.nextId
      state.waiting.append(Entry(id: id, unit: unit, completion: completion))
      if state.draining { return (id, .none) }
      state.draining = true
      return (id, .drain)
    }
    switch action {
    case .none: break
    case .drain: Task.detached { [self] in await drain() }
    case .refuse(let error): completion(error)
    }
    return id
  }

  /// Returns when `unit` has left, or throws why it did not. The unit is queued before the first
  /// suspension, in the caller's isolation: callers on one actor are queued in the order of the calls.
  func send(_ unit: Unit, isolation: isolated (any Swift.Actor)? = #isolation) async throws {
    try Task.checkCancellation()
    let queued = OSAllocatedUnfairLock<UInt64?>(initialState: nil)
    try await withTaskCancellationHandler {
      try await withCheckedThrowingContinuation {
        (continuation: CheckedContinuation<Void, Error>) in
        let id = enqueue(unit) { error in
          if let error { continuation.resume(throwing: error) } else { continuation.resume() }
        }
        queued.withLock { $0 = id }
        if Task.isCancelled { cancel(id) }
      }
    } onCancel: {
      if let id = queued.withLock({ $0 }) { cancel(id) }
    }
  }

  /// Fails every unit still waiting, and every later one, with `error`. A unit already leaving is
  /// ended by the transport's own error.
  func fail(_ error: Error) {
    let entries = state.withLock { state -> [Entry] in
      if state.failure == nil { state.failure = error }
      defer { state.waiting.removeAll() }
      return state.waiting
    }
    for entry in entries { entry.completion(error) }
  }

  /// Removes the unit `id` if it still waits; nothing of it is sent.
  func cancel(_ id: UInt64) {
    let entry = state.withLock { state -> Entry? in
      guard let index = state.waiting.firstIndex(where: { $0.id == id }) else { return nil }
      return state.waiting.remove(at: index)
    }
    entry?.completion(CancellationError())
  }

  private func drain() async {
    while true {
      let entry = state.withLock { state -> Entry? in
        guard !state.waiting.isEmpty else {
          state.draining = false
          return nil
        }
        return state.waiting.removeFirst()
      }
      guard let entry else { return }
      do {
        if let failure = state.withLock({ $0.failure }) { throw failure }
        try await transmit(entry.unit)
        entry.completion(nil)
      } catch {
        let first = state.withLock { $0.failure == nil }
        fail(error)
        entry.completion(error)
        if first { onFailure(error) }
      }
    }
  }
}
