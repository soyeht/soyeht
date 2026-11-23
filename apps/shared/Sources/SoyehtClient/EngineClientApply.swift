import Foundation
import SoyehtProtocol

extension EngineClient {
  /// Applies what the reader queued, in order, giving the window a turn every
  /// `ClientPolicy.applyBudgetMs`; what is not applied when the connection ends fails its requests.
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
        // A caller resumed by this answer goes before the frames behind it (ADR 0173).
        if case .reply(let request, _) = item {
          await request.acknowledged.wait()
          sliceStart = clock.now
        } else if clock.now - sliceStart >= budget {
          await Task.yield()
          sliceStart = clock.now
        }
      }
    }
  }

  /// Fails the requests whose answers were read and will not be applied.
  private static func abandon(_ items: ArraySlice<Incoming>) {
    for case .reply(let request, _) in items {
      request.continuation.resume(throwing: ClientFailure.disconnected)
    }
  }

  func receive(_ incoming: Incoming) throws {
    switch incoming {
    case .failure(let error): throw error
    case .reply(let request, let message): try complete(request, with: message)
    case .message(let message): try receive(message: message)
    case .other(let frame) where frame.kind == FrameKind.attachment:
      try responseAttachments.accept(frame)
    case .other(let frame) where frame.kind == FrameKind.terminal:
      if let delivery = try terminalStreams.accept(frame) {
        if terminalStreams.isReplaying(delivery.paneId) { replayGate.progress(delivery.paneId) }
        deliverTerminalOutput(to: delivery.paneId, bytes: delivery.bytes, reset: false)
      }
    case .other: break
    }
  }

  /// Applies the answer to `request`, which the reader took out of `pending`, and resumes its
  /// caller.
  private func complete(_ request: PendingRequests.Entry, with message: JsonValue) throws {
    if message["error"] != .null {
      if responseAttachments.isPending {
        responseAttachments.reset()
        request.continuation.resume(throwing: ClientFailure.invalidFrame)
        throw ClientFailure.invalidFrame
      }
      request.continuation.resume(throwing: ClientFailure.remote(error: message))
      return
    }
    let result = message["result"]
    do {
      let reference = try responseAttachmentReference(method: request.method, result: result)
      let attachment = try reference.map {
        WebHostAttachment(reference: $0, bytes: try responseAttachments.commit($0))
      }
      if reference == nil, responseAttachments.isPending { throw ClientFailure.invalidFrame }
      if request.method == .stateSnapshot { try applySnapshot(result) }
      if request.method == .paneStreamSubscribe,
        let paneId = request.params["paneId"].string
      {
        let info = try result.decoded(StreamInfo.self)
        // A close event can overtake an in-flight subscribe response. Do not resurrect stream
        // state after the pane (or its terminal role) has already disappeared.
        if panes.contains(where: { $0.paneId == paneId && $0.mode == .terminal }) {
          try terminalStreams.install(paneId: paneId, info: info)
        }
      }
      request.continuation.resume(
        returning: EngineResponse(value: result, attachment: attachment)
      )
    } catch {
      request.continuation.resume(throwing: error)
      throw error
    }
  }
}
