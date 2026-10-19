import Foundation
import SoyehtProtocol

/// The partial updates of streamed replies waiting for the next display frame. Each one carries
/// the whole text so far, so only the newest of a message matters: the others are dropped
/// unseen. A reply that streams faster than the screen redraws then costs one transcript write
/// and one layout per frame, however many updates arrive, and a slow frame cannot queue a layout
/// per update behind it.
struct AgentPartialUpdates {
  struct Update {
    let paneId: String
    let message: TranscriptMessage
  }

  private struct Key: Hashable {
    let paneId: String
    let localId: String
  }

  private var latest: [Key: Update] = [:]
  private var order: [Key] = []
  /// How many updates were dropped because a newer one of the same message replaced them.
  private(set) var superseded = 0

  var isEmpty: Bool { latest.isEmpty }

  /// Replies and thoughts that are still arriving. Everything else (a finished message, a tool
  /// call, a plan) is applied at once, after the partials that came before it.
  static func isPartial(_ message: TranscriptMessage) -> Bool {
    guard !message.complete else { return false }
    return message.kind == .message || message.kind == .thought
  }

  /// Keeps `message` for the next frame; returns whether it is the first one waiting, which is
  /// when the caller starts the frame timer.
  mutating func offer(paneId: String, _ message: TranscriptMessage) -> Bool {
    let key = Key(paneId: paneId, localId: message.localId)
    let first = latest.isEmpty
    if latest.updateValue(Update(paneId: paneId, message: message), forKey: key) == nil {
      order.append(key)
    } else {
      superseded += 1
    }
    return first
  }

  /// The newest update of each message, in the order the messages were first offered.
  mutating func drain() -> [Update] {
    defer {
      latest.removeAll(keepingCapacity: true)
      order.removeAll(keepingCapacity: true)
    }
    return order.compactMap { latest[$0] }
  }
}

/// Applies partial updates once per display frame, on the main actor.
@MainActor final class AgentPartialFlusher {
  private var updates = AgentPartialUpdates()
  private var timer: Task<Void, Never>?
  private weak var client: EngineClient?
  /// How many times a batch was delivered.
  private(set) var deliveries = 0

  init(_ client: EngineClient) {
    self.client = client
  }

  var superseded: Int { updates.superseded }

  func offer(paneId: String, _ message: TranscriptMessage) {
    guard updates.offer(paneId: paneId, message) else { return }
    timer = Task { @MainActor [weak self] in
      do {
        try await Task.sleep(for: .milliseconds(ClientPolicy.agentPartialFlushMs))
      } catch { return }  // Cancelled by a flush that already delivered the batch.
      self?.flush()
    }
  }

  /// Delivers what is waiting now. Called before anything that must see the transcript as the
  /// engine left it: a later event, a transcript page, a closed pane.
  func flush() {
    timer?.cancel()
    timer = nil
    let batch = updates.drain()
    guard !batch.isEmpty else { return }
    deliveries += 1
    client?.applyPartialUpdates(batch)
  }
}
