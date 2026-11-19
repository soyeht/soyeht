import Foundation

/// One voice a provider can speak with. `language` is a BCP-47 tag.
public struct SpeechVoice: Identifiable, Hashable, Sendable {
  public let id: String
  public let name: String
  public let language: String

  public init(id: String, name: String, language: String) {
    self.id = id
    self.name = name
    self.language = language
  }
}

/// What one utterance needs besides its text.
public struct SpeechSynthesisOptions: Equatable, Sendable {
  /// A voice id of the provider, or nil for the provider's own default.
  public var voice: String?
  /// A BCP-47 tag, used when no voice is named and the provider chooses a voice by language.
  public var language: String?
  /// 1 is the provider's normal pace. Kept inside `speedRange`: a value outside it, or not a
  /// number, becomes the nearest end or 1.
  public var speed: Double {
    didSet { speed = Self.clamped(speed) }
  }

  /// The pace the providers accept: the shared audio endpoints take 0.25 to 4.
  public static let speedRange = 0.25...4.0

  public init(voice: String? = nil, language: String? = nil, speed: Double = 1) {
    self.voice = voice
    self.language = language
    self.speed = Self.clamped(speed)
  }

  private static func clamped(_ speed: Double) -> Double {
    speed.isNaN ? 1 : min(max(speed, speedRange.lowerBound), speedRange.upperBound)
  }
}

/// When a word or a phoneme starts in rendered speech, as the provider reports it. The mouth
/// runtime uses these when the provider has them (ADR 0166 section 6, tier 1) and finds the
/// timings in the audio itself when it has not.
public struct SpeechTiming: Equatable, Sendable {
  public enum Kind: Sendable {
    case word
    case phoneme
  }

  public let kind: Kind
  /// The word as the text writes it, or the phoneme the provider names.
  public let text: String
  /// Seconds from the start of the speech.
  public let start: Double

  public init(kind: Kind, text: String, start: Double) {
    self.kind = kind
    self.text = text
    self.start = start
  }
}

/// Speech a provider rendered: mono float samples at the provider's own sample rate. Nothing is
/// played here: the voice loop plays the samples and drives the avatar's mouth from them.
///
/// The whole utterance is rendered before it is returned. Whether a provider also streams
/// partial audio is slice 7's decision, when the voice loop needs it; this type does not.
public struct SynthesizedSpeech: Equatable, Sendable {
  public let samples: [Float]
  public let sampleRate: Int
  /// The provider's own word or phoneme timings, in order, or nil. Nil is the normal case for
  /// every provider: the cloud and local models report none, and none of the built-in system
  /// voices delivered marks on the Mac that was measured. A non-nil value from the system path
  /// is not yet confirmed against a real voice (nor the unit of its `byteSampleOffset`), so the
  /// caller falls back to finding the timings in the audio itself (the aligner).
  public let timings: [SpeechTiming]?

  public init(samples: [Float], sampleRate: Int, timings: [SpeechTiming]? = nil) {
    self.samples = samples
    self.sampleRate = sampleRate
    self.timings = timings
  }

  public var duration: Duration {
    sampleRate > 0 ? .seconds(Double(samples.count) / Double(sampleRate)) : .zero
  }
}

public enum SpeechSynthesisError: Error, LocalizedError, Equatable, Sendable {
  case emptyText
  case noVoice
  case noAudio
  /// The audio the provider returned is not one this client reads.
  case unsupportedAudio(String)

  public var errorDescription: String? {
    switch self {
    case .emptyText: "There is nothing to say."
    case .noVoice: "No voice is available for this language."
    case .noAudio: "The voice returned no audio."
    case .unsupportedAudio(let reason): "The voice returned audio that cannot be read: \(reason)"
    }
  }
}

/// The text-to-speech behind the avatar's voice, and the only interface a provider implements.
/// The catalogue (`SpeechSynthesisRegistry`) lists the providers; tests use a fake.
///
/// A provider renders one utterance, a sentence or two, and returns its audio. It honours task
/// cancellation. It does not play audio, ask for a permission or split a long text: the caller
/// hands it one utterance at a time.
public protocol SpeechSynthesizing: Sendable {
  func synthesize(_ text: String, options: SpeechSynthesisOptions) async throws
    -> SynthesizedSpeech
}

extension SpeechSynthesisError {
  /// The text of an utterance without the blanks around it; nothing to say is an error.
  package static func utterance(_ text: String) throws -> String {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else { throw SpeechSynthesisError.emptyText }
    return trimmed
  }
}
