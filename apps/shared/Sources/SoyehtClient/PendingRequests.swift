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
/// however long the main actor takes to apply it (ADR 0171).
final class PendingRequests: Sendable {
  struct Entry: Sendable {
    var method: RequestMethod
    var params: JsonValue
    var startedAtNs: UInt64
    var continuation: CheckedContinuation<EngineResponse, Error>
    var timeout: Task<Void, Never>?
    var delivery: ReplyDelivery
  }

  private let entries = OSAllocatedUnfairLock<[String: Entry]>(initialState: [:])

  /// The waiting request answered by `id`, now no longer waiting and with its deadline cancelled.
  func take(_ id: String) -> Entry? {
    let entry = entries.withLock { $0.removeValue(forKey: id) }
    entry?.timeout?.cancel()
    return entry
  }

  var count: Int { entries.withLock { $0.count } }

  /// Fails every waiting request, as a disconnect does.
  func failAll(_ error: Error) {
    let requests = entries.withLock { entries in
      defer { entries.removeAll() }
      return entries
    }
    for request in requests.values {
      request.timeout?.cancel()
      request.continuation.resume(throwing: error)
    }
  }

  static func deadline(
    method: RequestMethod, timeoutSeconds: Double,
    waitForTimeout: @escaping @Sendable (RequestMethod, Double) async throws -> Void,
    onTimeout: @escaping @Sendable (ClientFailure) -> Void
  ) -> Task<Void, Never> {
    Task {
      do { try await waitForTimeout(method, timeoutSeconds) } catch { return }
      onTimeout(.requestTimeout(method.rawValue))
    }
  }

  static func waitForTimeout(_: RequestMethod, _ seconds: Double) async throws {
    try await Task.sleep(for: .seconds(seconds))
  }

  /// Waits for the answer to a request that `send` puts on the wire under a fresh id. A request
  /// that outlives its deadline fails, is logged, and runs `onTimeout` (the caller closes the wire).
  func response(
    _ method: RequestMethod, _ params: JsonValue, timeoutSeconds: Double,
    delivery: ReplyDelivery = .ordered,
    waitForTimeout: @escaping @Sendable (RequestMethod, Double) async throws -> Void,
    onTimeout: @escaping @Sendable () -> Void,
    send: @escaping @Sendable (String) async throws -> Void
  ) async throws -> EngineResponse {
    let id = UUID().uuidString
    return try await withCheckedThrowingContinuation { continuation in
      entries.withLock {
        $0[id] = Entry(
          method: method, params: params, startedAtNs: DispatchTime.now().uptimeNanoseconds,
          continuation: continuation, timeout: nil, delivery: delivery)
      }
      // The deadline starts once the request is registered, so one that fires at once finds it.
      let timeout = Self.deadline(
        method: method, timeoutSeconds: timeoutSeconds, waitForTimeout: waitForTimeout
      ) { [weak self] failure in
        guard let request = self?.take(id) else { return }
        let elapsedNs = DispatchTime.now().uptimeNanoseconds - request.startedAtNs
        let diagnostic = EngineTimeoutDiagnostic(
          method: request.method, params: request.params, durationMs: elapsedNs / 1_000_000
        )
        engineTimeoutLogger.error("Engine RPC timeout: \(diagnostic.logLine, privacy: .public)")
        request.continuation.resume(throwing: failure)
        onTimeout()
      }
      let answered = entries.withLock { entries -> Bool in
        guard entries[id] != nil else { return true }
        entries[id]?.timeout = timeout
        return false
      }
      if answered {
        timeout.cancel()
        return
      }
      Task {
        do {
          try await send(id)
        } catch {
          if let request = self.take(id) { request.continuation.resume(throwing: error) }
        }
      }
    }
  }
}
