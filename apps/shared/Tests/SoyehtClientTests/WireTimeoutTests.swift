import Foundation
import Network
import Testing

@testable import SoyehtClient

private final class Once: @unchecked Sendable {
  private let lock = NSLock()
  private var used = false

  func first() -> Bool {
    lock.lock()
    defer { lock.unlock() }
    defer { used = true }
    return !used
  }
}

/// A TCP server that accepts connections and never answers, like a Mac whose address no
/// longer routes: the socket opens and nothing comes back.
private final class SilentServer: @unchecked Sendable {
  private let lock = NSLock()
  private var kept: [NWConnection] = []
  private let listener: NWListener

  init() throws {
    listener = try NWListener(using: .tcp, on: .any)
    listener.newConnectionHandler = { [weak self] connection in
      self?.keep(connection)
      connection.start(queue: .global())
    }
  }

  private func keep(_ connection: NWConnection) {
    lock.lock()
    kept.append(connection)
    lock.unlock()
  }

  /// Starts listening and returns the port. The state handler can run more than once; only the
  /// first state that settles the question answers.
  func start() async throws -> UInt16 {
    try await withCheckedThrowingContinuation { continuation in
      let once = Once()
      listener.stateUpdateHandler = { [listener] state in
        switch state {
        case .ready:
          if once.first() { continuation.resume(returning: listener.port?.rawValue ?? 0) }
        case .failed(let error):
          if once.first() { continuation.resume(throwing: error) }
        default: break
        }
      }
      listener.start(queue: .global())
    }
  }

  func stop() {
    listener.cancel()
    lock.lock()
    for connection in kept { connection.cancel() }
    lock.unlock()
  }
}

/// A wire to a server that accepts and stays silent; `body` runs with it open.
@MainActor private func withSilentWire(
  greetingTimeout: Duration, _ body: (Wire) async throws -> Void
) async throws {
  let server = try SilentServer()
  let port = try await server.start()
  defer { server.stop() }
  let wire = Wire(url: try #require(URL(string: "ws://127.0.0.1:\(port)")))
  defer { wire.close() }
  try await wire.open(greetingTimeout: greetingTimeout)
  try await body(wire)
}

@MainActor
@Test func aSocketThatNeverGreetsFailsWithATimeoutNotACancel() async throws {
  try await withSilentWire(greetingTimeout: .milliseconds(300)) { wire in
    do {
      _ = try await wire.next()
      Issue.record("A silent socket produced a frame")
    } catch ClientFailure.timeout {
      // The deadline closed the socket; the reason is the deadline, not "cancelled".
    } catch {
      Issue.record("Expected a timeout, got \(error.localizedDescription)")
    }
  }
}

@MainActor
@Test func aWireClosedWithAReasonReportsThatReasonToTheReader() async throws {
  try await withSilentWire(greetingTimeout: .seconds(30)) { wire in
    let reader = Task { try await wire.next() }
    try await Task.sleep(for: .milliseconds(200))
    wire.close(ClientFailure.requestTimeout("pair.begin"))
    do {
      _ = try await reader.value
      Issue.record("A closed wire produced a frame")
    } catch ClientFailure.requestTimeout(let method) {
      #expect(method == "pair.begin")
    } catch {
      Issue.record("Expected the close reason, got \(error.localizedDescription)")
    }
  }
}
