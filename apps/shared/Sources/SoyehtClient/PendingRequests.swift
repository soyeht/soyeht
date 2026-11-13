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

/// The requests sent and not yet answered: one continuation per request id, and the deadline
/// that fails it. `EngineClient` owns the wire; this owns what waits on it.
@MainActor
final class PendingRequests {
  struct Entry {
    var method: RequestMethod
    var params: JsonValue
    var startedAtNs: UInt64
    var continuation: CheckedContinuation<EngineResponse, Error>
    var timeout: Task<Void, Never>
  }

  private var entries: [String: Entry] = [:]

  /// The waiting request answered by `id`, now no longer waiting.
  func take(_ id: String) -> Entry? { entries.removeValue(forKey: id) }

  /// Fails every waiting request, as a disconnect does.
  func failAll(_ error: Error) {
    let requests = entries
    entries.removeAll()
    for request in requests.values {
      request.timeout.cancel()
      request.continuation.resume(throwing: error)
    }
  }

  static func deadline(
    method: RequestMethod, timeoutSeconds: Double,
    waitForTimeout: @escaping @Sendable (RequestMethod, Double) async throws -> Void,
    onTimeout: @escaping @MainActor @Sendable (ClientFailure) -> Void
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
    waitForTimeout: @escaping @Sendable (RequestMethod, Double) async throws -> Void,
    onTimeout: @escaping @MainActor () -> Void,
    send: @escaping @MainActor (String) async throws -> Void
  ) async throws -> EngineResponse {
    let id = UUID().uuidString
    return try await withCheckedThrowingContinuation { continuation in
      let timeout = Self.deadline(
        method: method, timeoutSeconds: timeoutSeconds, waitForTimeout: waitForTimeout
      ) { [weak self] failure in
        guard let request = self?.entries.removeValue(forKey: id) else { return }
        let elapsedNs = DispatchTime.now().uptimeNanoseconds - request.startedAtNs
        let diagnostic = EngineTimeoutDiagnostic(
          method: request.method, params: request.params, durationMs: elapsedNs / 1_000_000
        )
        engineTimeoutLogger.error("Engine RPC timeout: \(diagnostic.logLine, privacy: .public)")
        request.continuation.resume(throwing: failure)
        onTimeout()
      }
      entries[id] = Entry(
        method: method, params: params, startedAtNs: DispatchTime.now().uptimeNanoseconds,
        continuation: continuation, timeout: timeout)
      Task {
        do {
          try await send(id)
        } catch {
          if let request = self.entries.removeValue(forKey: id) {
            request.timeout.cancel()
            request.continuation.resume(throwing: error)
          }
        }
      }
    }
  }
}
