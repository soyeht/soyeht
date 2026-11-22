import Foundation
import SoyehtProtocol
import os

/// The raw input a person types into a terminal, on its way to the engine. The keys are queued
/// from the main actor with no suspension, and a task of no actor sends them: in order, in pieces
/// of `ClientPolicy.terminalInputChunkBytes`, each piece after the engine said how much of the
/// last one it took. Nothing here waits for the main actor, so keystrokes leave while the window
/// is busy (ADR 0171).
final class TerminalInputQueue: Sendable {
  private struct Entry: Sendable {
    let paneId: String
    let data: Data
  }

  private struct State {
    var items: [Entry] = []
    var bytes = 0
    var draining = false
  }

  /// Sends one piece and returns how many of its bytes the engine took.
  typealias Send = @Sendable (_ paneId: String, _ chunk: Data) async throws -> Int

  private let state = OSAllocatedUnfairLock(initialState: State())
  private let limitBytes: Int
  private let chunkBytes: Int
  private let send: Send
  private let failed: @Sendable (Error) -> Void

  /// `failed` runs when a piece cannot be sent; everything still queued is dropped with it.
  init(
    limitBytes: Int = inputQueueBytes, chunkBytes: Int = inputChunkBytes,
    send: @escaping Send, failed: @escaping @Sendable (Error) -> Void
  ) {
    self.limitBytes = limitBytes
    self.chunkBytes = chunkBytes
    self.send = send
    self.failed = failed
  }

  /// Queues `data` for `paneId`. `false` when more than the limit is waiting: nothing was queued.
  func enqueue(_ data: Data, paneId: String) -> Bool {
    let start = state.withLock { state -> Bool? in
      guard state.bytes + data.count <= limitBytes else { return nil }
      state.items.append(Entry(paneId: paneId, data: data))
      state.bytes += data.count
      defer { state.draining = true }
      return !state.draining
    }
    guard let start else { return false }
    if start { Task.detached { [self] in await drain() } }
    return true
  }

  /// What to send next: the first queued entry, with the entries right behind it for the same pane
  /// added while they fit in one piece. Keys typed while a piece is on its way go together in the
  /// next one, so the wait of a key is one round trip however slow the engine is, not one per key.
  private func next() -> Entry? {
    state.withLock { state in
      guard !state.items.isEmpty else {
        state.draining = false
        return nil
      }
      var entry = state.items.removeFirst()
      state.bytes -= entry.data.count
      while let following = state.items.first, following.paneId == entry.paneId,
        entry.data.count + following.data.count <= chunkBytes
      {
        entry = Entry(paneId: entry.paneId, data: entry.data + following.data)
        state.bytes -= following.data.count
        state.items.removeFirst()
      }
      return entry
    }
  }

  private func drain() async {
    while let entry = next() {
      do {
        var offset = 0
        while offset < entry.data.count {
          let chunk = entry.data.subdata(
            in: offset..<min(offset + chunkBytes, entry.data.count))
          let accepted = try await send(entry.paneId, chunk)
          guard accepted > 0, accepted <= chunk.count else { throw ClientFailure.invalidFrame }
          offset += accepted
        }
      } catch {
        state.withLock { state in
          state.items.removeAll()
          state.bytes = 0
          state.draining = false
        }
        failed(error)
        return
      }
    }
  }
}

extension TerminalInputQueue {
  /// The queue of a client: each piece is a `pane.input` request through `channel` whose answer
  /// resumes the sender at once (`ReplyDelivery.immediate`). Only connection loss is retried,
  /// with the same idempotency key, as `EngineClient.mutation` does, after `isConnected` says the
  /// client is connected again.
  static func sending(
    through channel: RequestChannel, limitBytes: Int = inputQueueBytes,
    chunkBytes: Int = inputChunkBytes, isConnected: @escaping @Sendable () async -> Bool,
    failed: @escaping @Sendable (Error) -> Void
  ) -> TerminalInputQueue {
    TerminalInputQueue(
      limitBytes: limitBytes, chunkBytes: chunkBytes,
      send: { paneId, chunk in
        let params = try JsonValue.encoded(
          PaneInputParams(
            paneId: paneId, bytes: chunk.base64EncodedString(),
            idempotencyKey: UUID().uuidString))
        for attempt in 0..<3 {
          do {
            let result = try await channel.response(
              .paneInput, params, timeoutSeconds: defaultRequestTimeout, delivery: .immediate,
              lane: .priority
            ).value
            return result["acceptedBytes"].int ?? 0
          } catch let error as ClientFailure where error.canRetry && attempt < 2 {
            for _ in 0..<120 {
              if await isConnected() { break }
              try await Task.sleep(for: .milliseconds(100))
            }
          }
        }
        throw ClientFailure.disconnected
      },
      failed: failed)
  }
}
