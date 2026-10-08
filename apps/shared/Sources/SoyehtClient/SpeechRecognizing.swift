import Foundation

/// What the speech recognizer reports while it listens.
public enum SpeechEvent: Equatable, Sendable {
  /// The best transcript so far; it replaces the previous one.
  case partial(String)
  /// The recognizer finished; this is the transcript to insert.
  case final(String)
  case failed(String)
}

public enum DictationAuthorization: Equatable, Sendable {
  case authorized
  case denied(String)
  case unavailable(String)
}

/// The speech engine behind dictation. The app uses Apple's Speech framework
/// (`AppleSpeechRecognizer`); tests use a fake that emits events on demand.
@MainActor public protocol SpeechRecognizing: AnyObject {
  /// Asks for microphone and speech recognition access. Called only when the person taps
  /// the mic, so the system prompts appear only then.
  func authorize() async -> DictationAuthorization
  func start(onEvent: @escaping @MainActor (SpeechEvent) -> Void) throws
  /// Stops listening; the recognizer then reports `.final`.
  func finish()
  func cancel()
}
