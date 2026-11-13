import SoyehtProtocol

/// Answers a `web.command` the engine sent over the connection, through the host the app
/// provides. The answer carries the command's id and only error kinds the contract closes.
@MainActor enum WebHostResponder {
  static func response(
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
      return WebHostResponse(message: refusal(id: id, kind: kind, message: message))
    } catch {
      return WebHostResponse(
        message: refusal(id: id, kind: .internal_error, message: "The web command failed."))
    }
  }
  private static func refusal(id: String, kind: ErrorKind, message: String) -> JsonValue {
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
