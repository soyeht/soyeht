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
/// in the priority lane skips the line (ADR 0173).
///
/// `enter` runs `start`, which puts the request on the wire, in the calling context when the
/// request may go out at once, so requests made one after the other are queued for sending in that
/// order; a request that waits is started by `finish` of the one before it.
final class RequestAdmission: Sendable {
  private struct Waiter {
    let id: String
    let start: @Sendable () -> Void
  }

  /// First in, first out, without moving what is left each time the first goes.
  private struct Line {
    private var items: [Waiter] = []
    private var head = 0

    mutating func append(_ waiter: Waiter) { items.append(waiter) }

    mutating func popFirst() -> Waiter? {
      guard head < items.count else { return nil }
      defer {
        head += 1
        if head >= 1024 && head * 2 >= items.count {
          items.removeFirst(head)
          head = 0
        }
      }
      return items[head]
    }
  }

  private struct State {
    var free: Int
    var line = Line()
    /// The ids in `line` that still wait: one that leaves before its turn is dropped here and
    /// skipped when the line reaches it.
    var queued: Set<String> = []
    var holding: Set<String> = []
    /// Waiters that took a slot and are not on the wire yet, and whether a thread is starting them.
    var toStart = Line()
    var starting = false
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
      if state.queued.isEmpty && state.free > 0 {
        state.free -= 1
        state.holding.insert(id)
        return true
      }
      state.line.append(Waiter(id: id, start: start))
      state.queued.insert(id)
      return false
    }
    if now { start() }
  }

  /// Request `id` no longer waits for the engine (answered, failed or timed out): its slot goes to
  /// the longest waiter, or it never started and leaves the line.
  func finish(_ id: String) {
    let run = state.withLock { state -> Bool in
      if state.holding.remove(id) != nil {
        var handed = false
        while let next = state.line.popFirst() {
          guard state.queued.remove(next.id) != nil else { continue }
          state.holding.insert(next.id)
          state.toStart.append(next)
          handed = true
          break
        }
        if !handed { state.free += 1 }
      } else {
        state.queued.remove(id)
      }
      if state.starting { return false }
      state.starting = true
      return true
    }
    guard run else { return }
    // One thread starts them, one after the other. A start that is refused at once (the wire is
    // closed) comes back through `finish`, and must not go one level deeper for every request in
    // the line: it finds the thread running and returns.
    while let next = state.withLock({ state -> Waiter? in
      guard let next = state.toStart.popFirst() else {
        state.starting = false
        return nil
      }
      return next
    }) {
      next.start()
    }
  }

  var waiting: Int { state.withLock { $0.queued.count } }
}
