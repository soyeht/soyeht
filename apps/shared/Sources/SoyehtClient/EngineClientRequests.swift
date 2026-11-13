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

/// Request plumbing: one pending continuation per request id, a deadline for each, and the
/// retry of a mutation across a reconnect.
extension EngineClient {
  struct Pending {
    var method: RequestMethod
    var params: JsonValue
    var startedAtNs: UInt64
    var continuation: CheckedContinuation<EngineResponse, Error>
    var timeout: Task<Void, Never>
  }

  struct EngineResponse: Sendable {
    var value: JsonValue
    var attachment: WebHostAttachment?
  }

  static func requestTimeoutDeadline(
    method: RequestMethod, timeoutSeconds: Double,
    waitForTimeout: @escaping @Sendable (RequestMethod, Double) async throws -> Void,
    onTimeout: @escaping @MainActor @Sendable (ClientFailure) -> Void
  ) -> Task<Void, Never> {
    Task {
      do { try await waitForTimeout(method, timeoutSeconds) } catch { return }
      onTimeout(.requestTimeout(method.rawValue))
    }
  }

  static func waitForRequestTimeout(_: RequestMethod, _ seconds: Double) async throws {
    try await Task.sleep(for: .seconds(seconds))
  }

  func request(
    _ method: RequestMethod, _ params: JsonValue, timeoutSeconds: Double = defaultRequestTimeout,
    attachments: [WebHostAttachment] = []
  ) async throws -> JsonValue {
    try await requestResponse(
      method, params, timeoutSeconds: timeoutSeconds, attachments: attachments
    ).value
  }
  func requestResponse(
    _ method: RequestMethod, _ params: JsonValue, timeoutSeconds: Double = defaultRequestTimeout,
    attachments: [WebHostAttachment] = []
  ) async throws -> EngineResponse {
    if let stubbed = try await stubbedResponse(method, params) { return stubbed }
    guard let wire else { throw ClientFailure.disconnected }
    return try await pendingResponse(
      method, params, timeoutSeconds: timeoutSeconds,
      waitForTimeout: Self.waitForRequestTimeout
    ) { id in
      try await wire.send(
        .object([
          "jsonrpc": .string("2.0"), "id": .string(id),
          "method": .string(method.rawValue), "params": params,
        ]), attachments: attachments)
    }
  }
  func pendingResponse(
    _ method: RequestMethod, _ params: JsonValue, timeoutSeconds: Double,
    waitForTimeout: @escaping @Sendable (RequestMethod, Double) async throws -> Void,
    send: @escaping @MainActor (String) async throws -> Void
  ) async throws -> EngineResponse {
    let id = UUID().uuidString
    return try await withCheckedThrowingContinuation { continuation in
      let timeout = Self.requestTimeoutDeadline(
        method: method, timeoutSeconds: timeoutSeconds, waitForTimeout: waitForTimeout
      ) { [weak self] failure in
        guard let request = self?.pending.removeValue(forKey: id) else { return }
        let elapsedNs = DispatchTime.now().uptimeNanoseconds - request.startedAtNs
        let diagnostic = EngineTimeoutDiagnostic(
          method: request.method, params: request.params, durationMs: elapsedNs / 1_000_000
        )
        engineTimeoutLogger.error("Engine RPC timeout: \(diagnostic.logLine, privacy: .public)")
        request.continuation.resume(throwing: failure)
        self?.wire?.close()
      }
      pending[id] = Pending(
        method: method, params: params, startedAtNs: DispatchTime.now().uptimeNanoseconds,
        continuation: continuation, timeout: timeout)
      Task {
        do {
          try await send(id)
        } catch {
          if let request = self.pending.removeValue(forKey: id) {
            request.timeout.cancel()
            request.continuation.resume(throwing: error)
          }
        }
      }
    }
  }
  /// Retry only connection loss, with the same mutation parameters and idempotency key.
  func mutation<T: Encodable>(
    _ method: RequestMethod, _ params: T, timeoutSeconds: Double = defaultRequestTimeout,
    attachments: [WebHostAttachment] = []
  ) async throws -> JsonValue {
    let value = try JsonValue.encoded(params)
    for attempt in 0..<3 {
      do {
        return try await request(
          method, value, timeoutSeconds: timeoutSeconds, attachments: attachments)
      } catch let error
        as ClientFailure
        where error.canRetry && attempt < 2
      {
        for _ in 0..<120 {
          if connected { break }
          try await Task.sleep(for: .milliseconds(100))
        }
      }
    }
    throw ClientFailure.disconnected
  }
}

/// The web host seam: the engine sends a `web.command` over the same connection and the
/// client answers it through the host the app provides.
extension EngineClient {
  func acceptWebHostRequest(_ message: JsonValue) throws -> Bool {
    guard let method = message["method"].string, let id = message["id"].string else {
      return false
    }
    guard method == RequestMethod.webCommand.rawValue, webHostTasks[id] == nil else {
      throw ClientFailure.invalidFrame
    }
    let command = try message["params"].decoded(WebCommandParams.self)
    guard webHostTasks.count < ProtocolLimits.webCommandWaiters else {
      throw ClientFailure.invalidFrame
    }
    let activeWire = wire
    let host = webHost
    webHostTasks[id] = Task { [weak self] in
      let response = await Self.webHostResponse(id: id, command: command, host: host)
      guard let self, self.wire === activeWire else { return }
      do { try await activeWire?.send(response) } catch { activeWire?.close() }
      self.webHostTasks.removeValue(forKey: id)
    }
    return true
  }
  static func webHostResponse(
    id: String, command: WebCommandParams, host: (any WebHostPort)?
  ) async -> WebHostResponse {
    do {
      guard let host else {
        throw ClientFailure.webHost(.internal_error, "The web host is not ready.")
      }
      let output = try await host.execute(command)
      guard output.result.paneId == command.paneId else {
        throw ClientFailure.webHost(.internal_error, "The web host changed pane identity.")
      }
      return WebHostResponse(
        message: .object([
          "jsonrpc": .string("2.0"), "id": .string(id),
          "result": try .encoded(output.result),
        ]), attachments: output.attachments)
    } catch let ClientFailure.webHost(kind, message) {
      return WebHostResponse(message: webHostError(id: id, kind: kind, message: message))
    } catch {
      return WebHostResponse(
        message: webHostError(id: id, kind: .internal_error, message: "The web command failed."))
    }
  }
  static func webHostError(id: String, kind: ErrorKind, message: String) -> JsonValue {
    let safe: (ErrorKind, Int64) =
      switch kind {
      case .stale_reference: (.stale_reference, -32025)
      case .human_took_over: (.human_took_over, -32026)
      case .navigation_refused: (.navigation_refused, -32028)
      default: (.internal_error, -32018)
      }
    let (safeKind, code) = safe
    let error = RpcError(code: code, message: message, data: ErrorData(kind: safeKind))
    return .object([
      "jsonrpc": .string("2.0"), "id": .string(id),
      "error": (try? .encoded(error)) ?? .null,
    ])
  }
}
