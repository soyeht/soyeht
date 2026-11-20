import Foundation
import SoyehtProtocol
import os

/// The connection's requests, usable from any actor: the wire of the connection that is open, and
/// the requests waiting on it. The main actor's `EngineClient` and the task that sends terminal
/// input (which never waits for the main actor) go out through the same one (ADR 0171).
final class RequestChannel: Sendable {
  let pending = PendingRequests()
  private let current = OSAllocatedUnfairLock<Wire?>(initialState: nil)

  var wire: Wire? { current.withLock { $0 } }

  func attach(_ wire: Wire?) { current.withLock { $0 = wire } }

  /// Sends `method` and waits for its answer. A request the engine does not answer within
  /// `timeoutSeconds` fails, and closes the connection it went out on.
  func response(
    _ method: RequestMethod, _ params: JsonValue, timeoutSeconds: Double,
    delivery: ReplyDelivery = .ordered, attachments: [WebHostAttachment] = [],
    waitForTimeout: @escaping @Sendable (RequestMethod, Double) async throws -> Void =
      PendingRequests.waitForTimeout
  ) async throws -> EngineResponse {
    guard let wire else { throw ClientFailure.disconnected }
    return try await pending.response(
      method, params, timeoutSeconds: timeoutSeconds, delivery: delivery,
      waitForTimeout: waitForTimeout, onTimeout: { wire.close() }
    ) { id in
      try await wire.send(
        .object([
          "jsonrpc": .string("2.0"), "id": .string(id),
          "method": .string(method.rawValue), "params": params,
        ]), attachments: attachments)
    }
  }
}
