import Foundation
@preconcurrency import KokoroTTS
import SoyehtClient

/// Kokoro-82M (an 82-million-parameter StyleTTS 2 model) as the FP16 CoreML bundle of
/// `aufklarer/Kokoro-82M-CoreML`, speaking English on this device. It is opt-in: it runs after a
/// one-time download that the person starts from the voice screen, and it needs no key and sends
/// nothing anywhere.
///
/// English only: its phonemes come from the pronunciation dictionaries and a small neural
/// grapheme-to-phoneme model of the bundle. The phonemizer other languages need (espeak-ng) is
/// GPL and is not linked (ADR 0149), so no other language is offered.
public enum KokoroEngine {
  /// The English voices of the bundle: `a` is American and `b` British, `f` female and `m` male.
  private static let voiceIDs: [(id: String, language: String)] = [
    ("af_heart", "en-US"), ("af_alloy", "en-US"), ("af_aoede", "en-US"),
    ("af_bella", "en-US"), ("af_jessica", "en-US"), ("af_kore", "en-US"),
    ("af_nicole", "en-US"), ("af_nova", "en-US"), ("af_river", "en-US"),
    ("af_sarah", "en-US"), ("af_sky", "en-US"), ("am_adam", "en-US"),
    ("am_echo", "en-US"), ("am_eric", "en-US"), ("am_fenrir", "en-US"),
    ("am_liam", "en-US"), ("am_michael", "en-US"), ("am_onyx", "en-US"),
    ("am_puck", "en-US"), ("am_santa", "en-US"), ("bf_alice", "en-GB"),
    ("bf_emma", "en-GB"), ("bf_isabella", "en-GB"), ("bf_lily", "en-GB"),
    ("bm_daniel", "en-GB"), ("bm_fable", "en-GB"), ("bm_george", "en-GB"),
    ("bm_lewis", "en-GB"),
  ]

  /// The silence between two pieces of one text.
  private static let gap = 0.12

  /// The model card states Apache-2.0 for the weights, the grapheme-to-phoneme models and the
  /// English dictionaries.
  static let license = ProviderLicense(
    name: "Apache-2.0", url: URL(string: "https://www.apache.org/licenses/LICENSE-2.0"),
    commercialUse: true)

  public static let provider = SpeechSynthesisProvider(
    id: "kokoro", name: "Kokoro (English)",
    summary: "Speaks English on this device, with no key and nothing sent anywhere.",
    platforms: Set(DictationPlatform.allCases),
    license: license,
    voices: { voices },
    model: DictationModelInfo(
      sizeDescription: "about 333 MB", licenseName: license.name, licenseURL: license.url,
      commercialUse: license.commercialUse == true,
      install: { report in try await KokoroModelHolder.shared.install(report: report) },
      isInstalled: { KokoroModelHolder.shared.isInstalled },
      remove: { try KokoroModelHolder.shared.remove() }),
    availability: availability,
    make: { KokoroSpeechSynthesizer(holder: .shared) })

  static var voices: [SpeechVoice] {
    voiceIDs.map { SpeechVoice(id: $0.id, name: displayName($0.id), language: $0.language) }
  }

  /// `af_heart` is "Heart", `bm_george` is "George".
  private static func displayName(_ id: String) -> String {
    id.split(separator: "_").last.map { $0.prefix(1).uppercased() + $0.dropFirst() } ?? id
  }

  /// The same rule as the other local model: an arm64 chip and, on iPhone and iPad, 6 GB.
  @Sendable static func availability() -> DictationAvailability {
    LocalModelAvailability.appleSiliconWithEnoughMemory()
  }

  /// The speech of the model for a text: one pass per piece, one after the other, with a short
  /// silence between pieces.
  struct KokoroSpeechSynthesizer: SpeechSynthesizing {
    enum Failure: LocalizedError {
      case notReady

      var errorDescription: String? { "The Kokoro voice is not ready yet." }
    }

    let holder: KokoroModelHolder

    func synthesize(_ text: String, options: SpeechSynthesisOptions) async throws
      -> SynthesizedSpeech
    {
      guard let model = holder.current else { throw Failure.notReady }
      let pieces = KokoroTextWindow.split(
        text, maxCharacters: KokoroTextWindow.limit(forSpeed: options.speed))
      guard !pieces.isEmpty else { throw SpeechSynthesisError.emptyText }
      let voice = options.voice ?? voiceIDs[0].id
      guard options.language.map({ $0.lowercased().hasPrefix("en") }) ?? true,
        model.availableVoices.contains(voice)
      else { throw SpeechSynthesisError.noVoice }

      let rate = KokoroTTSModel.outputSampleRate
      let silence = [Float](repeating: 0, count: Int(gap * Double(rate)))
      var samples: [Float] = []
      for piece in pieces {
        try Task.checkCancellation()
        let heard = try await KokoroInference.render(
          piece, voice: voice, speed: Float(options.speed), in: model)
        if !samples.isEmpty { samples += silence }
        samples += heard
      }
      guard !samples.isEmpty else { throw SpeechSynthesisError.noAudio }
      return SynthesizedSpeech(samples: samples, sampleRate: rate)
    }
  }
}
