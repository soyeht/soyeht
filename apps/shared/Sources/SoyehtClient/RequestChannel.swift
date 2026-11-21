import Foundation
import SoyehtProtocol
import os

/// The connection's requests, usable from any actor: the wire of the connection that is open, and
/// the requests waiting on it. The main actor's `EngineClient` and the task that sends terminal
/// input (which never waits for the main actor) go out through the same one (ADR 0171).
final class RequestChannel: Sendable {
  let pending = PendingRequests()
  private let slots = RequestSlots()
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
    // Input skips the line, and so does a request the engine is given longer than the usual wait
    // for (it holds its slot for a long time); every other request waits for a slot, and its
    // deadline starts when it holds one, so time spent waiting here is not time the engine failed
    // to answer.
    var release: (@Sendable () -> Void)?
    if delivery == .ordered, timeoutSeconds <= defaultRequestTimeout {
      try await slots.acquire()
      release = { [slots] in slots.release() }
    }
    let send: @Sendable (String) async throws -> Void = { id in
      try await wire.send(
        .object([
          "jsonrpc": .string("2.0"), "id": .string(id),
          "method": .string(method.rawValue), "params": params,
        ]), attachments: attachments)
    }
    return try await pending.response(
      method, params, timeoutSeconds: timeoutSeconds, delivery: delivery, release: release,
      waitForTimeout: waitForTimeout, onTimeout: { wire.close() }, send: send)
  }
}
