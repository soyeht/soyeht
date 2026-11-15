@preconcurrency import AVFoundation
import Foundation

/// Text to speech with the system voices. It renders the speech to samples through
/// `AVSpeechSynthesizer.write`: nothing is played here, and no permission is asked.
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
    let render = Render()
    // Kept alive until the render ends: the synthesizer stops when it is released.
    let synthesizer = AVSpeechSynthesizer()
    return try await withTaskCancellationHandler {
      try await withCheckedThrowingContinuation { continuation in
        render.begin(continuation)
        synthesizer.write(utterance) { buffer in render.receive(buffer) }
      }
    } onCancel: {
      render.cancel()
      Task { @MainActor in synthesizer.stopSpeaking(at: .immediate) }
    }
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

  /// Collects the buffers the system sends, on its own queue, and resumes the caller once with
  /// the whole speech. An empty buffer is the system's end marker.
  private final class Render: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<SynthesizedSpeech, any Error>?
    private var samples: [Float] = []
    private var sampleRate = 0
    private var cancelled = false

    func begin(_ continuation: CheckedContinuation<SynthesizedSpeech, any Error>) {
      let wasCancelled = lock.withLock {
        self.continuation = cancelled ? nil : continuation
        return cancelled
      }
      if wasCancelled { continuation.resume(throwing: CancellationError()) }
    }

    func receive(_ buffer: AVAudioBuffer) {
      guard let pcm = buffer as? AVAudioPCMBuffer else { return }
      if pcm.frameLength > 0 {
        let heard = AppleSpeechSynthesizer.samples(of: pcm)
        lock.withLock {
          samples.append(contentsOf: heard)
          sampleRate = Int(pcm.format.sampleRate)
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
        return (continuation, SynthesizedSpeech(samples: samples, sampleRate: sampleRate))
      }
      guard let pending else { return }
      if result.samples.isEmpty {
        pending.resume(throwing: SpeechSynthesisError.noAudio)
      } else {
        pending.resume(returning: result)
      }
    }
  }
}

extension SpeechSynthesisProvider {
  /// The system voices: the fallback of the catalogue, on every device and with no download.
  public static let apple = SpeechSynthesisProvider(
    id: "apple", name: "Apple voices",
    summary: "The voices of this device. Nothing is downloaded or sent anywhere.",
    platforms: Set(DictationPlatform.allCases),
    voices: { AppleSpeechSynthesizer.voices() },
    make: { AppleSpeechSynthesizer() })
}
