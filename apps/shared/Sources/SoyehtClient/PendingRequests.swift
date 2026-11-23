import Foundation
import SoyehtProtocol
import os

private let engineTimeoutLogger = Logger(
  subsystem: "com.soyeht.next", category: "engine-client-timeout")

struct EngineTimeoutDiagnostic: Equatable {
  let method: String
  let durationMs: UInt64
  let paneId: String?

  init(method: RequestMethod, params: JsonValue, durationMs: UInt64) {
    self.method = method.rawValue
    self.durationMs = durationMs
    self.paneId = params["paneId"].string.flatMap(UUID.init(uuidString:))?.uuidString.lowercased()
  }

  var logLine: String {
    "method=\(method) durationMs=\(durationMs) paneId=\(paneId ?? "none")"
  }
}

struct EngineResponse: Sendable {
  var value: JsonValue
  var attachment: WebHostAttachment?
}

/// Says that a caller is back on its actor: the apply loop, which resumed it, waits for this before
/// it applies the next frame, so the caller reads the state its answer left, whatever the priority
/// of the tasks involved (ADR 0173).
final class Acknowledgement: Sendable {
  private struct State {
    var fired = false
    var waiter: CheckedContinuation<Void, Never>?
  }

  private let state = OSAllocatedUnfairLock(initialState: State())

  func fire() {
    let waiter = state.withLock { state -> CheckedContinuation<Void, Never>? in
      state.fired = true
      defer { state.waiter = nil }
      return state.waiter
    }
    waiter?.resume()
  }

  func wait() async {
    await withCheckedContinuation { continuation in
      let fired = state.withLock { state -> Bool in
        if state.fired { return true }
        state.waiter = continuation
        return false
      }
      if fired { continuation.resume() }
    }
  }
}

/// Where the answer to a request is delivered from. An `ordered` answer is applied by the main
/// actor in the order the frames arrived, after the events that came before it, because the caller
/// reads state those events changed. An `immediate` answer resumes its request from the reader,
/// without waiting for the main actor: the request reads nothing the main actor holds (terminal
/// input, which is acknowledged byte for byte and nothing else).
enum ReplyDelivery: Sendable { case ordered, immediate }

/// The requests sent and not yet answered: one continuation per request id, and the deadline
/// that fails it. `EngineClient` owns the wire; this owns what waits on it.
///
/// It runs on no actor. The reader takes a request out of it the moment its answer is read, which
/// also cancels its deadline, so a deadline can never fire for an answer the app already holds,
/// however long the main actor takes to apply it (ADR 0173).
///
/// The engine serves one connection's requests one at a time, so a request can wait behind the
/// others the client sent. A deadline that passes while the engine has answered other requests
/// since this one was sent does not mean a silent engine: it waits another window, up to
/// `ClientPolicy.capsRequestWindows`, and then fails alone, leaving the connection open. Only a
/// connection that answered nothing for a whole window is closed.
final class PendingRequests: Sendable {
  struct Entry: Sendable {
    var method: RequestMethod
    var params: JsonValue
    var startedAtNs: UInt64
    var continuation: CheckedContinuation<EngineResponse, Error>
    var timeout: Task<Void, Never>?
    var delivery: ReplyDelivery
    /// Gives back the slot the request holds (`RequestSlots`), once it no longer waits.
    var release: (@Sendable () -> Void)?
    /// Fired when the caller of an `ordered` request is back on its actor with the answer.
    var acknowledged = Acknowledgement()
  }

  private struct State {
    var entries: [String: Entry] = [:]
    /// When the reader last took the answer to a request.
    var lastAnswerNs: UInt64 = 0
  }

  /// How a deadline that has passed is settled.
  private enum Expiry {
    case gone
    case wait
    case fail(Entry, silent: Bool)
  }

  private let state = OSAllocatedUnfairLock(initialState: State())

  private static func now() -> UInt64 { DispatchTime.now().uptimeNanoseconds }

  /// The waiting request answered by `id`, now no longer waiting and with its deadline cancelled.
  /// Called by the reader for every answer it reads, which is what tells the engine is alive.
  func answer(_ id: String) -> Entry? {
    let entry = state.withLock { state -> Entry? in
      guard let entry = state.entries.removeValue(forKey: id) else { return nil }
      state.lastAnswerNs = Self.now()
      return entry
    }
    entry?.timeout?.cancel()
    entry?.release?()
    return entry
  }

  private func take(_ id: String) -> Entry? {
    let entry = state.withLock { $0.entries.removeValue(forKey: id) }
    entry?.timeout?.cancel()
    entry?.release?()
    return entry
  }

  var count: Int { state.withLock { $0.entries.count } }

  /// Fails every waiting request, as a disconnect does.
  func failAll(_ error: Error) {
    let requests = state.withLock { state in
      defer { state.entries.removeAll() }
      return state.entries
    }
    for request in requests.values {
      request.timeout?.cancel()
      request.release?()
      request.continuation.resume(throwing: error)
    }
  }

  static func waitForTimeout(_: RequestMethod, _ seconds: Double) async throws {
    try await Task.sleep(for: .seconds(seconds))
  }

  private func expire(_ id: String, windowsLeft: Int, timeoutSeconds: Double) -> Expiry {
    state.withLock { state in
      guard let entry = state.entries[id] else { return .gone }
      let window = UInt64(timeoutSeconds * 1_000_000_000)
      let now = Self.now()
      let answering =
        state.lastAnswerNs > entry.startedAtNs && now &- state.lastAnswerNs < window
      if answering && windowsLeft > 1 { return .wait }
      state.entries.removeValue(forKey: id)
      return .fail(entry, silent: !answering)
    }
  }

  /// The deadline of request `id`: after `timeoutSeconds` the request fails, or waits another
  /// window when the engine is still answering others (`windowsLeft` more at most).
  private func deadline(
    _ id: String, method: RequestMethod, timeoutSeconds: Double, windowsLeft: Int,
    waitForTimeout: @escaping @Sendable (RequestMethod, Double) async throws -> Void,
    onTimeout: @escaping @Sendable () -> Void
  ) -> Task<Void, Never> {
    Task.detached { [weak self] in
      do { try await waitForTimeout(method, timeoutSeconds) } catch { return }
      guard let self else { return }
      switch self.expire(id, windowsLeft: windowsLeft, timeoutSeconds: timeoutSeconds) {
      case .gone: return
      case .wait:
        let next = self.deadline(
          id, method: method, timeoutSeconds: timeoutSeconds, windowsLeft: windowsLeft - 1,
          waitForTimeout: waitForTimeout, onTimeout: onTimeout)
        if !self.setDeadline(id, next) { next.cancel() }
      case .fail(let request, let silent):
        request.release?()
        let elapsedNs = Self.now() - request.startedAtNs
        let diagnostic = EngineTimeoutDiagnostic(
          method: request.method, params: request.params, durationMs: elapsedNs / 1_000_000
        )
        engineTimeoutLogger.error("Engine RPC timeout: \(diagnostic.logLine, privacy: .public)")
        request.continuation.resume(throwing: ClientFailure.requestTimeout(method.rawValue))
        if silent { onTimeout() }
      }
    }
  }

  /// Whether `id` still waits and now has `task` as its deadline.
  private func setDeadline(_ id: String, _ task: Task<Void, Never>) -> Bool {
    state.withLock { state in
      guard state.entries[id] != nil else { return false }
      state.entries[id]?.timeout = task
      return true
    }
  }

  /// Fails request `id`, which waits no longer.
  func fail(_ id: String, _ error: Error) {
    take(id)?.continuation.resume(throwing: error)
  }

  /// Waits for the answer to the request `id`. It is registered and its deadline started now, in
  /// the calling context (so that callers on the main actor resume on it, in the order of the
  /// answers), and `start` is called there too, once, to put the request on the wire; it must not
  /// block. A request that outlives its deadline fails and is logged; when the connection answered
  /// nothing meanwhile it runs `onTimeout` too (the caller closes the wire).
  func response(
    id: String = UUID().uuidString,
    _ method: RequestMethod, _ params: JsonValue, timeoutSeconds: Double,
    delivery: ReplyDelivery = .ordered, windows: Int = ClientPolicy.capsRequestWindows,
    release: (@Sendable () -> Void)? = nil,
    waitForTimeout: @escaping @Sendable (RequestMethod, Double) async throws -> Void,
    onTimeout: @escaping @Sendable () -> Void,
    isolation: isolated (any Swift.Actor)? = #isolation,
    start: (String) -> Void
  ) async throws -> EngineResponse {
    let acknowledged = Acknowledgement()
    // Whatever ends the wait, the caller is back on its actor here.
    defer { acknowledged.fire() }
    return try await withCheckedThrowingContinuation { continuation in
      state.withLock {
        $0.entries[id] = Entry(
          method: method, params: params, startedAtNs: Self.now(),
          continuation: continuation, timeout: nil, delivery: delivery, release: release,
          acknowledged: acknowledged)
      }
      // The deadline starts once the request is registered, so one that fires at once finds it.
      let timeout = deadline(
        id, method: method, timeoutSeconds: timeoutSeconds, windowsLeft: windows,
        waitForTimeout: waitForTimeout, onTimeout: onTimeout)
      guard setDeadline(id, timeout) else {
        timeout.cancel()
        return
      }
      start(id)
    }
  }
}
