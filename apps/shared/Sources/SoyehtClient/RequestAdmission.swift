import Foundation
import SoyehtProtocol
import os

/// Which line a request goes out in.
enum RequestLane: Sendable {
  /// Counted: at most `ClientPolicy.capsRequestsInFlight` wait for the engine at once.
  case normal
  /// Goes out at once and is not counted: terminal input, and a request the engine is given
  /// longer than the usual wait for (it can hold its place for a long time).
  case priority
}

/// How many requests the client lets wait for the engine at once. The engine serves one
/// connection's requests one at a time, each as slowly as its wait for the Core lock, so a burst of
/// the client's own requests (a queue refresh for every pane that changed) makes whatever is sent
/// after it wait for the whole burst: a key typed behind a hundred refreshes reached the engine 25
/// seconds late. The requests over the limit wait here, in the order they were made, and a request
/// in the priority lane skips the line (ADR 0171).
///
/// `enter` runs `start`, which puts the request on the wire, in the calling context when the
/// request may go out at once, so requests made one after the other are queued for sending in that
/// order; a request that waits is started by `finish` of the one before it.
final class RequestAdmission: Sendable {
  private struct Waiter {
    let id: String
    let start: @Sendable () -> Void
  }

  private struct State {
    var free: Int
    var waiting: [Waiter] = []
    var holding: Set<String> = []
  }

  private let state: OSAllocatedUnfairLock<State>

  init(_ count: Int = ClientPolicy.capsRequestsInFlight) {
    state = OSAllocatedUnfairLock(initialState: State(free: count))
  }

  /// Admits request `id` to its lane. `start` is called once, now or when a slot is free.
  func enter(id: String, lane: RequestLane, start: @escaping @Sendable () -> Void) {
    guard lane == .normal else {
      start()
      return
    }
    let now = state.withLock { state -> Bool in
      if state.waiting.isEmpty && state.free > 0 {
        state.free -= 1
        state.holding.insert(id)
        return true
      }
      state.waiting.append(Waiter(id: id, start: start))
      return false
    }
    if now { start() }
  }

  /// Request `id` no longer waits for the engine (answered, failed or timed out): its slot goes to
  /// the longest waiter, or it never started and leaves the line.
  func finish(_ id: String) {
    let next = state.withLock { state -> Waiter? in
      if state.holding.remove(id) != nil {
        guard !state.waiting.isEmpty else {
          state.free += 1
          return nil
        }
        let next = state.waiting.removeFirst()
        state.holding.insert(next.id)
        return next
      }
      if let index = state.waiting.firstIndex(where: { $0.id == id }) {
        state.waiting.remove(at: index)
      }
      return nil
    }
    next?.start()
  }

  var waiting: Int { state.withLock { $0.waiting.count } }
}
