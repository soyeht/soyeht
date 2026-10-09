import Foundation
@preconcurrency import NemotronStreamingASR
import SoyehtClient

/// Dictation with Nemotron: the microphone feeds a streaming session on this device, and the
/// transcript so far goes back as partial events.
@MainActor final class NemotronRecognizer: SpeechRecognizing {
  enum Failure: LocalizedError {
    case notReady

    var errorDescription: String? { "The Nemotron model is not ready yet." }
  }

  private let capture = MicrophoneCapture()
  private var audio: AsyncThrowingStream<[Float], Error>.Continuation?
  private var transcribing: Task<Void, Never>?

  func authorize() async -> DictationAuthorization {
    await MicrophoneCapture.authorization()
  }

  func start(
    _ options: SpeechSessionOptions, onEvent: @escaping @MainActor (SpeechEvent) -> Void
  ) throws {
    cancel()
    guard let model = NemotronModelHolder.shared.current else { throw Failure.notReady }
    let session = try model.createSession(
      language: options.language,
      wordBoosting: Self.boosting(options.vocabulary, in: model))
    let resampler = try AudioResampler(sampleRate: Double(model.config.sampleRate))
    let (samples, continuation) = AsyncThrowingStream.makeStream(of: [Float].self)
    try capture.start(Self.sink(resampler, continuation))
    audio = continuation
    transcribing = Task { [inference = NemotronInference(session)] in
      await Self.transcribe(samples, with: inference, onEvent: onEvent)
    }
  }

  func finish() {
    capture.stop()
    audio?.finish()
    audio = nil
  }

  func cancel() {
    capture.stop()
    audio?.finish()
    audio = nil
    transcribing?.cancel()
    transcribing = nil
  }

  /// Runs on the audio thread, so it is built outside the main actor. A buffer that cannot be
  /// converted ends the stream with the reason.
  nonisolated private static func sink(
    _ resampler: AudioResampler, _ continuation: AsyncThrowingStream<[Float], Error>.Continuation
  ) -> MicrophoneCapture.Sink {
    { buffer in
      do {
        continuation.yield(try resampler.samples(from: buffer))
      } catch {
        continuation.finish(throwing: error)
      }
    }
  }

  /// Feeds the audio through the session, reporting each transcript, and closes it when the
  /// audio ends. Nothing is reported once the dictation is cancelled.
  private static func transcribe(
    _ samples: AsyncThrowingStream<[Float], Error>, with inference: NemotronInference,
    onEvent: @MainActor (SpeechEvent) -> Void
  ) async {
    do {
      for try await chunk in samples {
        if let text = try await inference.push(chunk), !Task.isCancelled {
          onEvent(.partial(text))
        }
      }
      let closed = try await inference.finalize()
      if !Task.isCancelled { onEvent(.final(closed ?? "")) }
    } catch {
      if !Task.isCancelled { onEvent(.failed(error.localizedDescription)) }
    }
  }

  /// Phrases the model should favor. The model suggests how hard each one needs to be pushed:
  /// a term it already spells in one piece needs a nudge, a fragmented name needs a shove.
  /// Without a tokenizer model in the bundle, boosting silently does nothing, so none is set.
  private static func boosting(
    _ vocabulary: [String], in model: NemotronStreamingASRModel
  ) -> WordBoostingConfig? {
    guard !vocabulary.isEmpty, model.wordBoostingTokenizerStatus.mode != .vocabFallback else {
      return nil
    }
    let suggestions = model.wordBoostingSuggestions(for: vocabulary)
    return WordBoostingConfig(
      phrases: suggestions.map { .init($0.phrase, boost: $0.suggestedBoost) })
  }
}
