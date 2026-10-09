import Foundation

/// What the speech recognizer reports while it listens.
public enum SpeechEvent: Equatable, Sendable {
  /// The best transcript so far; it replaces the previous one.
  case partial(String)
  /// The recognizer finished; this is the transcript to insert.
  case final(String)
  case failed(String)
}

extension SpeechEvent {
  /// What a recognizer callback carries, as one event: a transcript wins over an error.
  init?(text: String?, isFinal: Bool, failure: String?) {
    if let text {
      self = isFinal ? .final(text) : .partial(text)
    } else if let failure {
      self = .failed(failure)
    } else {
      return nil
    }
  }
}

public enum DictationAuthorization: Equatable, Sendable {
  case authorized
  case denied(String)
  case unavailable(String)
}

/// What one dictation session needs besides the audio.
public struct SpeechSessionOptions: Equatable, Sendable {
  /// A BCP-47 tag, or nil to let the engine choose its own default or detect the language.
  public var language: String?
  /// Words to favor; empty for engines that ignore a vocabulary.
  public var vocabulary: [String]

  public init(language: String? = nil, vocabulary: [String] = []) {
    self.language = language
    self.vocabulary = vocabulary
  }
}

/// The speech engine behind dictation. The app uses Apple's Speech framework
/// (`AppleSpeechRecognizer`) or another engine of the catalogue (`DictationEngineRegistry`);
/// tests use a fake that emits events on demand. This is the only interface an engine has to
/// implement.
@MainActor public protocol SpeechRecognizing: AnyObject {
  /// Asks for microphone and speech recognition access. Called only when the person taps
  /// the mic, so the system prompts appear only then.
  func authorize() async -> DictationAuthorization
  func start(
    _ options: SpeechSessionOptions, onEvent: @escaping @MainActor (SpeechEvent) -> Void
  ) throws
  /// Stops listening; the recognizer then reports `.final`.
  func finish()
  func cancel()
}
