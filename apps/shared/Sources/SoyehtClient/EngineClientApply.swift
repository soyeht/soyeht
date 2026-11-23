import Foundation
import SoyehtProtocol

extension EngineClient {
  /// Applies what the reader queued, in the order it was read, on this actor. The window gets a
  /// turn between slices of `ClientPolicy.applyBudgetMs`, so a long backlog (the main thread was
  /// busy for seconds) does not become a longer one. What is not applied when the connection ends
  /// fails its requests, which no one else would resume.
  func apply(_ queue: IncomingQueue, from wire: Wire) async {
    let budget = Duration.milliseconds(ClientPolicy.applyBudgetMs)
    let clock = ContinuousClock()
    while true {
      let batch = await queue.next()
      if batch.isEmpty {
        // The queue ended because the backlog passed its limit: say so, and resynchronize.
        if let error = queue.failure, self.wire === wire {
          connectionText = error.localizedDescription
          disconnect()
        }
        return
      }
      var sliceStart = clock.now
      for (index, item) in batch.enumerated() {
        guard self.wire === wire else {
          Self.abandon(batch[index...])
          return
        }
        do {
          try receive(item)
        } catch {
          connectionText = error.localizedDescription
          disconnect()
          Self.abandon(batch[(index + 1)...])
          return
        }
        // A caller resumed by this answer goes before the frames behind it: it reads the state the
        // answer and everything before it left, as it did when the reader ran on this actor.
        if case .reply = item {
          await Task.yield()
          sliceStart = clock.now
        } else if clock.now - sliceStart >= budget {
          await Task.yield()
          sliceStart = clock.now
        }
      }
    }
  }

  /// Fails the requests whose answers were read and will not be applied: no one else would resume
  /// them.
  private static func abandon(_ items: ArraySlice<Incoming>) {
    for case .reply(let request, _) in items {
      request.continuation.resume(throwing: ClientFailure.disconnected)
    }
  }
}
