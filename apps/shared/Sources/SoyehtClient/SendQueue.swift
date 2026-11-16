import Foundation

/// One serial queue for everything a connection sends. A request that carries binary attachments
/// leaves as several frames followed by the request, and the engine commits those frames to "the
/// following request" (`AttachmentAssembler`), so the whole unit must leave with nothing between
/// its frames. The queue sends one unit at a time, in the order the units were queued, whatever the
/// transport: a WebSocket takes one message per frame and each takes a suspension, a local socket
/// takes one write.
///
/// Cancellation: a unit still waiting is removed and nothing of it is sent. A unit already
/// leaving runs to its end, because stopping after the frames and before the request would leave
/// them pending on the engine, which then refuses every request until one commits them. A
/// transport failure inside a unit ends the connection for the same reason: the units behind it
/// fail with that error and `onFailure` closes the wire, so the engine drops the half unit with the
/// connection.
@MainActor
final class SendQueue<Unit: Sendable> {
  private final class Entry {
    let id: UInt64
    let unit: Unit
    let continuation: CheckedContinuation<Void, Error>

    init(id: UInt64, unit: Unit, continuation: CheckedContinuation<Void, Error>) {
      self.id = id
      self.unit = unit
      self.continuation = continuation
    }
  }

  private let transmit: @MainActor (Unit) async throws -> Void
  private let onFailure: @MainActor (Error) -> Void
  private var waiting: [Entry] = []
  private var draining = false
  private var failure: Error?
  private var nextId: UInt64 = 0

  /// `transmit` puts one whole unit on the transport; `onFailure` runs once, when it throws.
  init(
    transmit: @escaping @MainActor (Unit) async throws -> Void,
    onFailure: @escaping @MainActor (Error) -> Void
  ) {
    self.transmit = transmit
    self.onFailure = onFailure
  }

  /// Returns when `unit` has left, or throws why it did not.
  func send(_ unit: Unit) async throws {
    if let failure { throw failure }
    try Task.checkCancellation()
    nextId &+= 1
    let id = nextId
    try await withTaskCancellationHandler {
      try await withCheckedThrowingContinuation { continuation in
        waiting.append(Entry(id: id, unit: unit, continuation: continuation))
        drain()
      }
    } onCancel: {
      Task { @MainActor in self.cancel(id) }
    }
  }

  /// Fails every unit still waiting, and every later one, with `error`. A unit already leaving is
  /// ended by the transport's own error.
  func fail(_ error: Error) {
    if failure == nil { failure = error }
    let entries = waiting
    waiting.removeAll()
    for entry in entries { entry.continuation.resume(throwing: error) }
  }

  private func cancel(_ id: UInt64) {
    guard let index = waiting.firstIndex(where: { $0.id == id }) else { return }
    waiting.remove(at: index).continuation.resume(throwing: CancellationError())
  }

  private func drain() {
    guard !draining else { return }
    draining = true
    Task { @MainActor in
      while !waiting.isEmpty {
        let entry = waiting.removeFirst()
        do {
          if let failure { throw failure }
          try await transmit(entry.unit)
          entry.continuation.resume()
        } catch {
          let first = failure == nil
          fail(error)
          entry.continuation.resume(throwing: error)
          if first { onFailure(error) }
        }
      }
      draining = false
    }
  }
}
