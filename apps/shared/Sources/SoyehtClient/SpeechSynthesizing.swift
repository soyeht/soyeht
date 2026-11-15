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
  /// 1 is the provider's normal pace.
  public var speed: Double

  public init(voice: String? = nil, language: String? = nil, speed: Double = 1) {
    self.voice = voice
    self.language = language
    self.speed = speed
  }
}

/// Speech a provider rendered: mono float samples at the provider's own sample rate. Nothing is
/// played here: the voice loop plays the samples and drives the avatar's mouth from them.
public struct SynthesizedSpeech: Equatable, Sendable {
  public let samples: [Float]
  public let sampleRate: Int

  public init(samples: [Float], sampleRate: Int) {
    self.samples = samples
    self.sampleRate = sampleRate
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
  static func utterance(_ text: String) throws -> String {
    let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty else { throw SpeechSynthesisError.emptyText }
    return trimmed
  }
}
