@preconcurrency import AVFoundation
import Foundation

/// Text to speech with the system voices. It renders the speech to samples through
/// `AVSpeechSynthesizer.write`: nothing is played here, and no permission is asked. Word and
/// phoneme marks, when the system sends them, become the speech's timings; the built-in voices
/// tried on the Mac sent none, so the conversion is tested with constructed marks only.
@MainActor public final class AppleSpeechSynthesizer: SpeechSynthesizing {
  nonisolated public init() {}

  public func synthesize(_ text: String, options: SpeechSynthesisOptions) async throws
    -> SynthesizedSpeech
  {
    let trimmed = try SpeechSynthesisError.utterance(text)
    guard let voice = Self.voice(for: options) else { throw SpeechSynthesisError.noVoice }
    let utterance = AVSpeechUtterance(string: trimmed)
    utterance.voice = voice
    utterance.rate = Self.rate(forSpeed: options.speed)
    let render = Render(text: trimmed)
    // Kept alive until the render ends: the synthesizer stops when it is released.
    nonisolated(unsafe) let synthesizer = AVSpeechSynthesizer()
    return try await withTaskCancellationHandler {
      try await withCheckedThrowingContinuation { continuation in
        render.begin(continuation)
        synthesizer.write(
          utterance, toBufferCallback: Self.buffers(into: render),
          toMarkerCallback: Self.marks(into: render))
      }
    } onCancel: {
      render.cancel()
      Task { @MainActor in synthesizer.stopSpeaking(at: .immediate) }
    }
  }

  /// The system calls these on a queue of its own: they are built outside the main actor and
  /// never inherit it.
  nonisolated private static func buffers(into render: Render) -> @Sendable (AVAudioBuffer) -> Void
  {
    { buffer in render.receive(buffer) }
  }

  nonisolated private static func marks(
    into render: Render
  ) -> @Sendable ([AVSpeechSynthesisMarker]) -> Void {
    { marks in render.receive(marks) }
  }

  /// The voices of the device, the ones of the person's language first.
  nonisolated public static func voices() -> [SpeechVoice] {
    let current = Locale.current.identifier.replacingOccurrences(of: "_", with: "-")
    return AVSpeechSynthesisVoice.speechVoices()
      .map { SpeechVoice(id: $0.identifier, name: $0.name, language: $0.language) }
      .sorted { lhs, rhs in
        let (left, right) = (lhs.language == current, rhs.language == current)
        if left != right { return left }
        return (lhs.language, lhs.name) < (rhs.language, rhs.name)
      }
  }

  nonisolated private static func voice(for options: SpeechSynthesisOptions)
    -> AVSpeechSynthesisVoice?
  {
    if let id = options.voice { return AVSpeechSynthesisVoice(identifier: id) }
    return AVSpeechSynthesisVoice(language: options.language ?? Locale.current.identifier)
  }

  /// The system's rate runs from its minimum to its maximum with the default in between, and
  /// is not linear in speed; scaling the default is close enough for 0.5x to 2x.
  nonisolated static func rate(forSpeed speed: Double) -> Float {
    let scaled = Float(speed) * AVSpeechUtteranceDefaultSpeechRate
    return min(max(scaled, AVSpeechUtteranceMinimumSpeechRate), AVSpeechUtteranceMaximumSpeechRate)
  }

  /// Mono samples of a rendered buffer, whichever of the two sample types the system used.
  nonisolated static func samples(of buffer: AVAudioPCMBuffer) -> [Float] {
    let count = Int(buffer.frameLength)
    if let channel = buffer.floatChannelData?[0] {
      return Array(UnsafeBufferPointer(start: channel, count: count))
    }
    if let channel = buffer.int16ChannelData?[0] {
      return UnsafeBufferPointer(start: channel, count: count).map { Float($0) / 32_768 }
    }
    return []
  }

  /// Collects the buffers and marks the system sends, on its own queue, and resumes the caller
  /// once with the whole speech. An empty buffer is the system's end marker.
  private final class Render: @unchecked Sendable {
    private let lock = NSLock()
    private let text: String
    private var continuation: CheckedContinuation<SynthesizedSpeech, any Error>?
    private var samples: [Float] = []
    private var sampleRate = 0
    private var bytesPerFrame = 0
    private var marks: [AVSpeechSynthesisMarker] = []
    private var cancelled = false

    init(text: String) { self.text = text }

    func begin(_ continuation: CheckedContinuation<SynthesizedSpeech, any Error>) {
      let wasCancelled = lock.withLock {
        self.continuation = cancelled ? nil : continuation
        return cancelled
      }
      if wasCancelled { continuation.resume(throwing: CancellationError()) }
    }

    func receive(_ marks: [AVSpeechSynthesisMarker]) {
      lock.withLock { self.marks += marks }
    }

    func receive(_ buffer: AVAudioBuffer) {
      guard let pcm = buffer as? AVAudioPCMBuffer else { return }
      if pcm.frameLength > 0 {
        let heard = AppleSpeechSynthesizer.samples(of: pcm)
        lock.withLock {
          samples.append(contentsOf: heard)
          sampleRate = Int(pcm.format.sampleRate)
          bytesPerFrame = Int(pcm.format.streamDescription.pointee.mBytesPerFrame)
        }
        return
      }
      finish()
    }

    func cancel() {
      let pending = lock.withLock {
        cancelled = true
        defer { continuation = nil }
        return continuation
      }
      pending?.resume(throwing: CancellationError())
    }

    private func finish() {
      let (pending, result) = lock.withLock {
        defer { continuation = nil }
        return (continuation, speech())
      }
      guard let pending else { return }
      if result.samples.isEmpty {
        pending.resume(throwing: SpeechSynthesisError.noAudio)
      } else {
        pending.resume(returning: result)
      }
    }

    /// The speech so far. Call with the lock held.
    private func speech() -> SynthesizedSpeech {
      SynthesizedSpeech(
        samples: samples, sampleRate: sampleRate,
        timings: AppleSpeechSynthesizer.timings(
          of: marks, in: text, sampleRate: sampleRate, bytesPerFrame: bytesPerFrame))
    }
  }

  /// The word and phoneme marks as timings, in order, or nil when the system sent none.
  /// `byteSampleOffset` counts bytes of the rendered audio from its start.
  nonisolated static func timings(
    of marks: [AVSpeechSynthesisMarker], in text: String, sampleRate: Int, bytesPerFrame: Int
  ) -> [SpeechTiming]? {
    guard sampleRate > 0, bytesPerFrame > 0 else { return nil }
    let characters = text as NSString
    let timings: [SpeechTiming] = marks.compactMap { mark in
      let start = Double(mark.byteSampleOffset) / Double(bytesPerFrame * sampleRate)
      switch mark.mark {
      case .word:
        let range = mark.textRange
        guard range.location != NSNotFound, NSMaxRange(range) <= characters.length else {
          return nil
        }
        return SpeechTiming(kind: .word, text: characters.substring(with: range), start: start)
      case .phoneme:
        return SpeechTiming(kind: .phoneme, text: mark.phoneme, start: start)
      default:
        return nil
      }
    }
    return timings.isEmpty ? nil : timings.sorted { $0.start < $1.start }
  }
}

extension SpeechSynthesisProvider {
  /// The system voices: the fallback of the catalogue, on every device and with no download.
  public static let apple = SpeechSynthesisProvider(
    id: "apple", name: "Apple voices",
    summary: "The voices of this device. Nothing is downloaded or sent anywhere.",
    platforms: Set(DictationPlatform.allCases),
    license: ProviderLicense(name: "Part of the operating system", commercialUse: true),
    voices: { AppleSpeechSynthesizer.voices() },
    make: { AppleSpeechSynthesizer() })
}
