import Foundation
import SoyehtProtocol
import os

/// The connection's requests, usable from any actor: the wire of the connection that is open, and
/// the requests waiting on it. The main actor's `EngineClient` and the task that sends terminal
/// input (which never waits for the main actor) go out through the same one (ADR 0171).
final class RequestChannel: Sendable {
  let pending = PendingRequests()
  private let admission = RequestAdmission()
  private let current = OSAllocatedUnfairLock<Wire?>(initialState: nil)

  var wire: Wire? { current.withLock { $0 } }

  func attach(_ wire: Wire?) { current.withLock { $0 = wire } }

  /// Sends `method` and waits for its answer. The request is registered and queued for sending at
  /// the moment of the call, in the caller's isolation: requests made one after the other reach
  /// the engine in that order, and a caller on the main actor resumes on it with the apply loop
  /// waiting its turn. A request the engine does not answer within `timeoutSeconds` fails, and
  /// closes the connection it went out on when that connection answered nothing meanwhile.
  func response(
    _ method: RequestMethod, _ params: JsonValue, timeoutSeconds: Double,
    delivery: ReplyDelivery = .ordered, lane: RequestLane = .normal,
    attachments: [WebHostAttachment] = [],
    waitForTimeout: @escaping @Sendable (RequestMethod, Double) async throws -> Void =
      PendingRequests.waitForTimeout,
    isolation: isolated (any Swift.Actor)? = #isolation
  ) async throws -> EngineResponse {
    guard let wire else { throw ClientFailure.disconnected }
    let id = UUID().uuidString
    let unit = try wire.unit(
      for: .object([
        "jsonrpc": .string("2.0"), "id": .string(id),
        "method": .string(method.rawValue), "params": params,
      ]), attachments: attachments)
    let (pending, admission) = (pending, admission)
    return try await pending.response(
      id: id, method, params, timeoutSeconds: timeoutSeconds, delivery: delivery,
      release: { admission.finish(id) },
      waitForTimeout: waitForTimeout, onTimeout: { wire.close() }, isolation: isolation
    ) { id in
      admission.enter(id: id, lane: lane) {
        wire.enqueue(unit) { error in
          if let error { pending.fail(id, error) }
        }
      }
    }
  }
}
