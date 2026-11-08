import Foundation
import SoyehtProtocol

/// What a terminal receives for one display frame: the bytes that arrived since the last frame,
/// joined, and whether they start on a screen that is replayed from its beginning.
struct TerminalOutputBatch: Equatable {
  var reset: Bool
  var bytes: [UInt8]
}

/// Joins the chunks of a terminal's output into one delivery per display frame.
///
/// The engine sends a pane's output in many small chunks (a program that streams fast sends
/// hundreds a second). Feeding each one to the emulator parses it, wipes its search index and
/// asks for a redraw, so the cost grew with the number of chunks instead of with the bytes. Here
/// the chunks wait for the next frame and are fed together: the emulator parses the same bytes in
/// the same order, once per frame.
///
/// A pane that streams faster than anyone reads (a build log, `cat` of a big file) is fed and
/// drawn less often: when a frame carried `ClientPolicy.terminalOutputFloodBytes` or more, the
/// next one waits `terminalOutputFloodMs`. Every row of a scrolling screen is drawn again, and
/// shaping the text of a row is what a redraw costs, so the time that goes to drawing falls
/// with the rate of the frames while the bytes and their order stay the same.
///
/// Two things are never delayed: output right after the person typed (the echo of a key, which
/// is the latency the person feels), and a backlog past `ClientPolicy.terminalOutputFlushBytes`,
/// which would hold memory for no gain. Whatever waits is delivered by `flush()` before a
/// surface goes away, so no byte is lost.
@MainActor
final class TerminalOutputCoalescer {
  typealias Deliver = (TerminalOutputBatch) -> Void
  /// Arranges one call of the action after the given number of milliseconds.
  typealias FrameTimer = @MainActor (Int, @escaping @MainActor () -> Void) -> Void

  private let deliver: Deliver
  private let scheduleFrame: FrameTimer
  private let now: () -> ContinuousClock.Instant
  private var batches: [TerminalOutputBatch] = []
  private var waitingBytes = 0
  private var frameGeneration: UInt64 = 0
  private var frameScheduled = false
  private var streaming = false
  private var interactiveUntil: ContinuousClock.Instant?
  /// How many deliveries were made, and how many chunks they carried: the first is one per
  /// frame however many chunks arrive.
  private(set) var deliveries = 0
  private(set) var chunks = 0

  init(
    now: @escaping () -> ContinuousClock.Instant = { .now },
    scheduleFrame: @escaping FrameTimer = TerminalOutputCoalescer.sleepOneFrame,
    deliver: @escaping Deliver
  ) {
    self.now = now
    self.scheduleFrame = scheduleFrame
    self.deliver = deliver
  }

  var isEmpty: Bool { batches.isEmpty }

  /// The person typed: the output of the next moments is delivered as it arrives.
  func noteInput() {
    interactiveUntil = now().advanced(by: .milliseconds(ClientPolicy.terminalInteractiveWindowMs))
  }

  /// Keeps a chunk for the next frame, after the chunks that came before it. A `reset` starts
  /// a new batch, since the screen is replayed from its beginning there.
  func enqueue(_ bytes: Data, reset: Bool) {
    chunks += 1
    if reset || batches.isEmpty {
      batches.append(TerminalOutputBatch(reset: reset, bytes: Array(bytes)))
    } else {
      batches[batches.count - 1].bytes.append(contentsOf: bytes)
    }
    waitingBytes += bytes.count
    if isInteractive || waitingBytes >= ClientPolicy.terminalOutputFlushBytes {
      flush()
    } else if !frameScheduled {
      frameScheduled = true
      let generation = frameGeneration
      let wait = streaming ? ClientPolicy.terminalOutputFloodMs : ClientPolicy.terminalOutputFlushMs
      scheduleFrame(wait) { [weak self] in
        guard let self, self.frameGeneration == generation else { return }
        self.flush()
      }
    }
  }

  /// Delivers what waits now. Called by the frame, and before anything that must see the screen
  /// as the engine left it.
  func flush() {
    frameGeneration &+= 1
    frameScheduled = false
    guard !batches.isEmpty else { return }
    let ready = batches
    batches.removeAll(keepingCapacity: true)
    streaming = waitingBytes >= ClientPolicy.terminalOutputFloodBytes
    waitingBytes = 0
    for batch in ready {
      deliveries += 1
      deliver(batch)
    }
  }

  private var isInteractive: Bool {
    guard let interactiveUntil else { return false }
    return now() < interactiveUntil
  }

  /// One frame by the clock.
  static func sleepOneFrame(_ milliseconds: Int, _ action: @escaping @MainActor () -> Void) {
    Task { @MainActor in
      do {
        try await Task.sleep(for: .milliseconds(milliseconds))
      } catch { return }  // Never cancelled; a cancelled sleep delivers on the next flush.
      action()
    }
  }
}
