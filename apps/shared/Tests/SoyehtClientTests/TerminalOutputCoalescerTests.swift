import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// A frame clock the test advances by hand: the coalescer asks for one tick, the test runs it.
@MainActor private final class Frames {
  var waiting: [@MainActor () -> Void] = []
  var waits: [Int] = []
  var instant = ContinuousClock.now

  func timer(_ milliseconds: Int, _ action: @escaping @MainActor () -> Void) {
    waits.append(milliseconds)
    waiting.append(action)
  }
  func now() -> ContinuousClock.Instant { instant }

  /// The next frame arrives: every tick that was asked for runs.
  func tick() {
    let due = waiting
    waiting = []
    instant = instant.advanced(by: .milliseconds(ClientPolicy.terminalOutputFlushMs))
    for action in due { action() }
  }
}

@MainActor private final class Screen {
  var batches: [TerminalOutputBatch] = []
  var bytes: [UInt8] { batches.flatMap(\.bytes) }
}

@MainActor private func rig() -> (Frames, Screen, TerminalOutputCoalescer) {
  let frames = Frames()
  let screen = Screen()
  let output = TerminalOutputCoalescer(now: frames.now, scheduleFrame: frames.timer) {
    screen.batches.append($0)
  }
  return (frames, screen, output)
}

@MainActor @Test func chunksWithinOneFrameAreFedOnceInTheirOrder() {
  let (frames, screen, output) = rig()

  for index in 0..<500 { output.enqueue(Data([UInt8(index % 256)]), reset: false) }

  #expect(screen.batches.isEmpty)
  #expect(frames.waiting.count == 1)
  frames.tick()
  #expect(screen.batches.count == 1)
  #expect(screen.bytes == (0..<500).map { UInt8($0 % 256) })
  #expect(output.chunks == 500)
  #expect(output.deliveries == 1)
  #expect(output.isEmpty)
}

@MainActor @Test func eachFrameFeedsOnlyWhatArrivedSinceTheLast() {
  let (frames, screen, output) = rig()

  output.enqueue(Data([1, 2]), reset: false)
  output.enqueue(Data([3]), reset: false)
  frames.tick()
  output.enqueue(Data([4]), reset: false)
  frames.tick()
  frames.tick()

  #expect(screen.batches.map(\.bytes) == [[1, 2, 3], [4]])
}

@MainActor @Test func aReplayStartsANewBatchAndKeepsTheOrderAroundIt() {
  let (frames, screen, output) = rig()

  output.enqueue(Data([1]), reset: false)
  output.enqueue(Data([2]), reset: true)
  output.enqueue(Data([3]), reset: false)
  output.enqueue(Data(), reset: true)
  output.enqueue(Data([4]), reset: false)
  frames.tick()

  #expect(
    screen.batches == [
      TerminalOutputBatch(reset: false, bytes: [1]),
      TerminalOutputBatch(reset: true, bytes: [2, 3]),
      TerminalOutputBatch(reset: true, bytes: [4]),
    ])
}

@MainActor @Test func flushDeliversWhatWaitsAndTheLateFrameFeedsNothingTwice() {
  let (frames, screen, output) = rig()

  output.enqueue(Data([1]), reset: false)
  output.flush()
  #expect(screen.bytes == [1])

  // The frame that was asked for before the flush finds nothing; the next chunk asks again.
  output.enqueue(Data([2]), reset: false)
  frames.tick()
  #expect(screen.bytes == [1, 2])
  #expect(screen.batches.count == 2)
  output.flush()
  #expect(screen.batches.count == 2)
}

@MainActor @Test func theEchoOfATypedKeyIsNotHeldForTheFrame() {
  let (frames, screen, output) = rig()

  output.noteInput()
  output.enqueue(Data([65]), reset: false)
  #expect(screen.bytes == [65])

  frames.instant = frames.instant.advanced(
    by: .milliseconds(ClientPolicy.terminalInteractiveWindowMs + 1))
  output.enqueue(Data([66]), reset: false)
  #expect(screen.bytes == [65])
  frames.tick()
  #expect(screen.bytes == [65, 66])
}

@MainActor @Test func aPaneThatFloodsIsFedLessOftenUntilItQuietens() {
  let (frames, screen, output) = rig()
  let flood = Data(repeating: 1, count: ClientPolicy.terminalOutputFloodBytes)

  output.enqueue(Data([1]), reset: false)
  frames.tick()
  output.enqueue(flood, reset: false)
  frames.tick()
  output.enqueue(Data([2]), reset: false)
  frames.tick()
  output.enqueue(Data([3]), reset: false)
  frames.tick()
  output.enqueue(Data([4]), reset: false)

  #expect(
    frames.waits == [
      ClientPolicy.terminalOutputFlushMs, ClientPolicy.terminalOutputFlushMs,
      ClientPolicy.terminalOutputFloodMs, ClientPolicy.terminalOutputFlushMs,
      ClientPolicy.terminalOutputFlushMs,
    ])
  #expect(screen.batches.count == 4)
}

@MainActor @Test func aBacklogPastTheLimitIsFedAtOnce() {
  let (frames, screen, output) = rig()
  let half = Data(repeating: 7, count: ClientPolicy.terminalOutputFlushBytes / 2)

  output.enqueue(half, reset: false)
  #expect(screen.batches.isEmpty)
  output.enqueue(half, reset: false)

  #expect(screen.bytes.count == ClientPolicy.terminalOutputFlushBytes)
  #expect(screen.batches.count == 1)
}

// MARK: The client

@MainActor private func startReplay(_ client: EngineClient, _ paneId: String) throws {
  try installStream(client, paneId)
  _ = try client.terminalStreams.start(
    StreamReplayStartEvent(
      streamId: "stream-\(paneId)", generation: 1, fromOffset: "0", toOffset: "0", reset: true))
}

@MainActor @Test func outputDoesNotInvalidateAViewThatShowsAGapNotice() throws {
  let client = EngineClient(socket: "/tmp/sn-terminal-output-observation/engine.sock")
  try startReplay(client, "a")
  try startReplay(client, "b")
  var offset: UInt64 = 0

  let chunkFires = fires(
    watching: {
      _ = client.gapText(for: "a")
      _ = client.gapText(for: "b")
    },
    whenChanging: {
      for _ in 0..<50 {
        do {
          _ = try client.terminalStreams.accept(
            terminal(reference: 97, offset: offset, bytes: Data([65])))
        } catch {
          Issue.record("a chunk was refused: \(error)")
        }
        offset += 1
      }
    })
  #expect(!chunkFires)

  let gapFires = fires(
    watching: { _ = client.gapText(for: "a") },
    whenChanging: {
      do {
        _ = try client.terminalStreams.gap(
          StreamGapEvent(streamId: "stream-a", fromOffset: "50", toOffset: "60"))
      } catch {
        Issue.record("the gap was refused: \(error)")
      }
    })
  #expect(gapFires)
  #expect(client.gapText(for: "a") == TerminalStatePresentation.recordedGap.text)
  #expect(client.gapText(for: "b") == nil)

  client.terminalStreams.remove("a")
  #expect(client.gapText(for: "a") == nil)
}
