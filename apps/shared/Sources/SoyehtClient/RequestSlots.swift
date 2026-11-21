import Foundation
import SoyehtProtocol
import os

/// How many requests the client lets wait for the engine at once. The engine serves one
/// connection's requests one at a time, each as slowly as its wait for the Core lock, so a burst
/// of the client's own requests (a queue refresh for every pane that changed) makes whatever is
/// sent after it wait for the whole burst: a key typed behind a hundred refreshes reached the
/// engine 25 seconds late. The requests over the limit wait here, in order, and terminal input,
/// which skips the line, waits for at most the ones already sent (ADR 0171).
final class RequestSlots: Sendable {
  private struct Waiter {
    let id: UInt64
    let continuation: CheckedContinuation<Void, Error>
  }

  private struct State {
    var free: Int
    var waiters: [Waiter] = []
    var nextId: UInt64 = 0
  }

  private enum Place { case granted, queued, cancelled }

  private let state: OSAllocatedUnfairLock<State>

  init(_ count: Int = ClientPolicy.capsRequestsInFlight) {
    state = OSAllocatedUnfairLock(initialState: State(free: count))
  }

  /// Returns when a slot is held; `release()` gives it back. Throws when the caller is cancelled
  /// while it waits, holding nothing.
  func acquire() async throws {
    try Task.checkCancellation()
    let id = state.withLock { state -> UInt64 in
      state.nextId &+= 1
      return state.nextId
    }
    try await withTaskCancellationHandler {
      try await withCheckedThrowingContinuation {
        (continuation: CheckedContinuation<Void, Error>) in
        let place = state.withLock { state -> Place in
          if Task.isCancelled { return .cancelled }
          if state.free > 0 {
            state.free -= 1
            return .granted
          }
          state.waiters.append(Waiter(id: id, continuation: continuation))
          return .queued
        }
        switch place {
        case .granted: continuation.resume()
        case .cancelled: continuation.resume(throwing: CancellationError())
        case .queued: break
        }
      }
    } onCancel: {
      let waiter = state.withLock { state -> Waiter? in
        guard let index = state.waiters.firstIndex(where: { $0.id == id }) else { return nil }
        return state.waiters.remove(at: index)
      }
      waiter?.continuation.resume(throwing: CancellationError())
    }
  }

  /// Gives back a slot: the longest waiter takes it, or it is free again.
  func release() {
    let next = state.withLock { state -> Waiter? in
      if state.waiters.isEmpty {
        state.free += 1
        return nil
      }
      return state.waiters.removeFirst()
    }
    next?.continuation.resume()
  }

  var waiting: Int { state.withLock { $0.waiters.count } }
}
