import Foundation
import SoyehtProtocol

extension ClientFailure {
  /// The failure an answer or a connection-level error object stands for.
  static func remote(error message: JsonValue) -> ClientFailure {
    .remote(
      message["error"]["data"]["kind"].string ?? ErrorKind.internal_error.rawValue,
      message["error"]["message"].string ?? "Request refused",
      retryAfterSec: message["error"]["data"]["retryAfterSec"].int.flatMap(UInt32.init))
  }
}

/// Reads one connection and feeds the main actor, on no actor of the app (ADR 0173).
///
/// It frames, parses, and matches the answer to a request with the request that waits for it the
/// moment the answer is read, so a request's deadline and the main actor's work never meet: an
/// answer that has arrived has left the deadline's reach, and an answer the main actor has not
/// applied yet is not a silent engine. Everything else goes to `IncomingQueue` in the order it was
/// read, for the main actor to apply.
enum EngineReader {
  /// Reads until the connection ends or `queue` is full, then queues the failure that ended it.
  static func run(wire: Wire, pending: PendingRequests, queue: IncomingQueue) async {
    do {
      while !Task.isCancelled {
        let frame = try await wire.next()
        let weight = frame.header.count + frame.payload.count
        let incoming: Incoming
        if frame.kind == FrameKind.json {
          let message = try JSONDecoder().decode(JsonValue.self, from: frame.payload)
          guard let routed = route(message, pending: pending) else { continue }
          incoming = routed
        } else {
          incoming = .other(frame)
        }
        guard queue.push(incoming, bytes: weight) else {
          // The main actor is further behind than the app keeps: resynchronize.
          queue.finish(failing: ClientFailure.invalidStream)
          wire.close(ClientFailure.invalidStream)
          return
        }
      }
    } catch {
      _ = queue.push(.failure(error))
    }
  }

  /// The answer of a request that wants it at once is delivered here; any other message is
  /// returned to be queued for the main actor.
  private static func route(_ message: JsonValue, pending: PendingRequests) -> Incoming? {
    guard message["jsonrpc"].string == "2.0", let id = message["id"].string,
      let request = pending.answer(id)
    else { return .message(message) }
    switch request.delivery {
    case .ordered: return .reply(request, message)
    case .immediate:
      if message["error"] != .null {
        request.continuation.resume(throwing: ClientFailure.remote(error: message))
      } else {
        request.continuation.resume(
          returning: EngineResponse(value: message["result"], attachment: nil))
      }
      return nil
    }
  }
}
