import AVFoundation
import Foundation
@preconcurrency import Speech

/// Dictation through Apple's Speech framework, on the device when the recognizer supports
/// it. The microphone and speech recognition prompts appear on the first tap of the mic.
@MainActor public final class AppleSpeechRecognizer: SpeechRecognizing {
  private let capture = MicrophoneCapture()
  private var request: SFSpeechAudioBufferRecognitionRequest?
  private var task: SFSpeechRecognitionTask?

  public init() {}

  public func authorize() async -> DictationAuthorization {
    let speech = await SystemCallback.value { SFSpeechRecognizer.requestAuthorization($0) }
    guard speech == .authorized else {
      return .denied("Allow speech recognition for Soyeht in System Settings to dictate.")
    }
    let microphone = await MicrophoneCapture.authorization()
    guard microphone == .authorized else { return microphone }
    guard Self.recognizer(for: nil)?.isAvailable == true else {
      return .unavailable("Dictation is not available for this language right now.")
    }
    return .authorized
  }

  public func start(
    _ options: SpeechSessionOptions, onEvent: @escaping @MainActor (SpeechEvent) -> Void
  ) throws {
    cancel()
    guard let recognizer = Self.recognizer(for: options.language) else {
      throw DictationError.unavailable
    }
    let request = SFSpeechAudioBufferRecognitionRequest()
    request.shouldReportPartialResults = true
    if recognizer.supportsOnDeviceRecognition { request.requiresOnDeviceRecognition = true }
    try capture.start(Self.feed(request))
    self.request = request
    task = recognizer.recognitionTask(with: request, resultHandler: Self.report(to: onEvent))
  }

  /// A recognizer for the tag, or for the device's language when there is no tag.
  private static func recognizer(for language: String?) -> SFSpeechRecognizer? {
    SFSpeechRecognizer(locale: language.map(Locale.init(identifier:)) ?? .current)
  }

  /// The audio tap runs on the audio thread, so it is built outside the main actor.
  nonisolated static func feed(
    _ request: SFSpeechAudioBufferRecognitionRequest
  ) -> MicrophoneCapture.Sink {
    { buffer in request.append(buffer) }
  }

  /// Results arrive on the recognizer's queue and hop to the main actor.
  nonisolated static func report(
    to onEvent: @escaping @MainActor (SpeechEvent) -> Void
  ) -> @Sendable (SFSpeechRecognitionResult?, (any Error)?) -> Void {
    { result, error in
      let text = result?.bestTranscription.formattedString
      let isFinal = result?.isFinal ?? false
      let failure = error?.localizedDescription
      Task { @MainActor in
        if let event = SpeechEvent(text: text, isFinal: isFinal, failure: failure) {
          onEvent(event)
        }
      }
    }
  }

  public func finish() {
    capture.stop()
    request?.endAudio()
  }

  public func cancel() {
    capture.stop()
    task?.cancel()
    task = nil
    request = nil
  }
}

enum DictationError: LocalizedError {
  case unavailable

  var errorDescription: String? { "Dictation is not available on this device." }
}

extension DictationEngine {
  /// Apple's Speech framework: no download, every device, the fallback of the catalogue.
  public static let apple = DictationEngine(
    id: "apple", name: "Apple Speech", badge: "Apple",
    summary: "Built in. Works everywhere, on this device when the language allows it.",
    platforms: Set(DictationPlatform.allCases),
    languages: [.system]
      + [
        "pt-BR", "pt-PT", "en-US", "en-GB", "es-ES", "es-MX", "fr-FR", "de-DE", "it-IT",
        "ja-JP", "zh-CN", "zh-TW",
      ].map(DictationLanguage.init(tag:)),
    make: { AppleSpeechRecognizer() })
}
