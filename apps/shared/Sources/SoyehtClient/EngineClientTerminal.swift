import Foundation
import SoyehtProtocol

/// Terminal panes: the output sinks, the stream subscription, keyboard input and resize.
extension EngineClient {
  struct TerminalOutputTarget {
    var owner: ObjectIdentifier
    var sink: (Data, Bool) -> Void
  }

  func subscribe(_ pane: String) async throws {
    guard !terminalSubscriptionsInFlight.contains(pane) else { return }
    terminalSubscriptionsInFlight.insert(pane)
    defer { terminalSubscriptionsInFlight.remove(pane) }
    _ = try await request(
      .paneStreamSubscribe, .object(["paneId": .string(pane), "from": .string("tail")]))
  }
  public func refreshTerminal() {
    guard let pane = selectedPaneId, selectedPane?.mode == .terminal, connected else { return }
    perform { try await self.subscribe(pane) }
  }
  public func refreshTerminal(_ paneId: String) {
    guard connected, panes.contains(where: { $0.paneId == paneId && $0.mode == .terminal })
    else { return }
    // macOS subscribes every terminal after the snapshot. A surface mounted after that must
    // consume the existing stream instead of replacing it and replaying the tail a second time.
    guard !terminalStreams.contains(paneId) else { return }
    perform { try await self.subscribe(paneId) }
  }
  public func gapText(for paneId: String) -> String? { terminalGapTexts[paneId] }
  func claimTerminalOutput(
    owner: AnyObject, sink: @escaping (Data, Bool) -> Void
  ) {
    // SwiftUI may install a replacement before dismantling the prior surface.
    // The owner makes a late teardown unable to clear the replacement sink.
    terminalOutputOwner = ObjectIdentifier(owner)
    terminalOutputSink = sink
  }
  func releaseTerminalOutput(owner: AnyObject) {
    guard terminalOutputOwner == ObjectIdentifier(owner) else { return }
    terminalOutputOwner = nil
    terminalOutputSink = nil
  }
  func claimTerminalOutput(
    paneId: String, owner: AnyObject, sink: @escaping (Data, Bool) -> Void
  ) {
    terminalOutputSinks[paneId] = TerminalOutputTarget(
      owner: ObjectIdentifier(owner), sink: sink)
  }
  /// Whether `owner` held the pane's sink, which is now released.
  @discardableResult func releaseTerminalOutput(paneId: String, owner: AnyObject) -> Bool {
    guard terminalOutputSinks[paneId]?.owner == ObjectIdentifier(owner) else { return false }
    return terminalOutputSinks.removeValue(forKey: paneId) != nil
  }
  func deliverTerminalOutput(to paneId: String, bytes: Data, reset: Bool) {
    terminalOutputSinks[paneId]?.sink(bytes, reset)
    if paneId == selectedPaneId { terminalOutputSink?(bytes, reset) }
  }
  var terminalOutputSinkCount: Int { terminalOutputSinks.count }

  func discardTerminalPane(_ paneId: String) {
    terminalStreams.remove(paneId)
    terminalSubscriptionsInFlight.remove(paneId)
    terminalOutputSinks.removeValue(forKey: paneId)
    resizeTasks.removeValue(forKey: paneId)?.cancel()
    resizeGenerations.removeValue(forKey: paneId)
    #if os(macOS)
      terminalViewRegistry.remove(paneId)
    #endif
  }

  public func input(_ data: Data) {
    guard let pane = selectedPaneId else { return }
    input(data, paneId: pane)
  }
  func input(_ data: Data, paneId pane: String) {
    guard connected, selectedPaneId == pane,
      panes.contains(where: { $0.paneId == pane && $0.mode == .terminal }), !data.isEmpty
    else { return }
    guard queuedInputBytes + data.count <= inputQueueBytes else {
      problem = "Input is too large. Wait for the keyboard queue to empty."
      return
    }
    inputQueue.append((pane, data))
    queuedInputBytes += data.count
    guard inputTask == nil else { return }
    inputTask = Task {
      defer { inputTask = nil }
      while !inputQueue.isEmpty {
        let (pane, data) = inputQueue.removeFirst()
        queuedInputBytes -= data.count
        do {
          var offset = 0
          while offset < data.count {
            let chunk = data.subdata(in: offset..<min(offset + inputChunkBytes, data.count))
            let result = try await mutation(
              .paneInput,
              PaneInputParams(
                paneId: pane,
                bytes: chunk.base64EncodedString(), idempotencyKey: UUID().uuidString))
            guard let accepted = result["acceptedBytes"].int, accepted > 0, accepted <= chunk.count
            else {
              throw ClientFailure.invalidFrame
            }
            offset += accepted
          }
        } catch {
          problem = error.localizedDescription
          inputQueue.removeAll()
          queuedInputBytes = 0
          break
        }
      }
    }
  }
  public func resize(cols: Int, rows: Int) {
    guard let pane = selectedPaneId else { return }
    resize(paneId: pane, cols: cols, rows: rows)
  }
  func resize(paneId pane: String, cols: Int, rows: Int) {
    guard connected, panes.contains(where: { $0.paneId == pane && $0.mode == .terminal }),
      cols > 0, rows > 0
    else { return }
    resizeTasks[pane]?.cancel()
    let generation = resizeGenerations[pane, default: 0] &+ 1
    resizeGenerations[pane] = generation
    resizeTasks[pane] = Task {
      do {
        try await Task.sleep(for: .milliseconds(100))
        _ = try await mutation(
          .paneResize,
          PaneResizeParams(
            paneId: pane,
            cols: UInt32(min(cols, 1000)), rows: UInt32(min(rows, 1000)),
            idempotencyKey: UUID().uuidString))
      } catch is CancellationError {} catch { problem = error.localizedDescription }
      if resizeGenerations[pane] == generation {
        resizeTasks.removeValue(forKey: pane)
        resizeGenerations.removeValue(forKey: pane)
      }
    }
  }
}
