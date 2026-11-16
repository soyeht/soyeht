import Foundation

/// The microphone's samples cut into frames and read by the voice-activity model, off the main
/// actor: a model's prediction 31 times a second has no place there. One serial queue owns the
/// framing, the model and the last eight seconds of audio (a ring buffer), and the main actor gets
/// the results.
final class VoiceFrontEnd: @unchecked Sendable {
  struct Frame: Equatable, Sendable {
    /// The time the frame's audio ended, in milliseconds on the microphone's clock.
    let at: Int
    let probability: Double
    /// The loudness of the frame, 0 to 1.
    let level: Float
  }

  struct Result: Sendable {
    var frames: [Frame] = []
    /// Set once, when the model failed and the loudness detector took over for the rest of the stream.
    var failure: String?
  }

  private let queue = DispatchQueue(label: "com.soyeht.voice.frontend")
  private var detector: any VoiceActivityDetecting
  private var fellBack = false
  private var pending: [Float] = []
  private var ring = [Float](repeating: 0, count: VoiceFrames.turnWindow)
  private var written = 0
  private var seen = 0

  init(detector: any VoiceActivityDetecting) { self.detector = detector }

  /// Starts a stream: the detector forgets the last one and the clock is back at 0.
  func reset(detector fresh: (any VoiceActivityDetecting)? = nil) async {
    await run {
      if let fresh { self.detector = fresh }
      self.detector.reset()
      self.fellBack = false
      self.pending = []
      self.written = 0
      self.seen = 0
    }
  }

  /// The frames that `samples` completes, with the model's verdict on each.
  func process(_ samples: [Float]) async -> Result {
    await run {
      var result = Result()
      self.pending += samples
      var offset = 0
      while self.pending.count - offset >= VoiceFrames.size {
        let frame = Array(self.pending[offset..<(offset + VoiceFrames.size)])
        offset += VoiceFrames.size
        self.append(frame)
        result.frames.append(
          Frame(
            at: self.seen * 1_000 / VoiceFrames.sampleRate, probability: self.read(frame, &result),
            level: SpeechEnvelope.rms(of: frame[...])))
      }
      self.pending.removeFirst(offset)
      return result
    }
  }

  /// The last eight seconds of the turn, oldest first.
  func window() async -> [Float] {
    await run {
      let count = min(self.written, VoiceFrames.turnWindow)
      let start = (self.written - count) % VoiceFrames.turnWindow
      return (0..<count).map { self.ring[(start + $0) % VoiceFrames.turnWindow] }
    }
  }

  private func append(_ frame: [Float]) {
    for sample in frame {
      ring[written % VoiceFrames.turnWindow] = sample
      written += 1
    }
    seen += frame.count
  }

  private func read(_ frame: [Float], _ result: inout Result) -> Double {
    do {
      return try detector.probability(of: frame)
    } catch {
      guard !fellBack else { return 0 }
      fellBack = true
      result.failure = error.localizedDescription
      let energy = EnergyVoiceActivity()
      detector = energy
      return energy.probability(of: frame)
    }
  }

  private func run<Value: Sendable>(_ work: @escaping @Sendable () -> Value) async -> Value {
    await withCheckedContinuation { continuation in
      queue.async { continuation.resume(returning: work()) }
    }
  }
}
