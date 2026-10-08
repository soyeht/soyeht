import AVFoundation
import Foundation
@preconcurrency import Speech

/// Dictation through Apple's Speech framework, on the device when the recognizer supports
/// it. The microphone and speech recognition prompts appear on the first tap of the mic.
@MainActor public final class AppleSpeechRecognizer: SpeechRecognizing {
  private let locale: Locale
  private var engine: AVAudioEngine?
  private var request: SFSpeechAudioBufferRecognitionRequest?
  private var task: SFSpeechRecognitionTask?

  public init(locale: Locale = .current) {
    self.locale = locale
  }

  public func authorize() async -> DictationAuthorization {
    let speech = await withCheckedContinuation { continuation in
      SFSpeechRecognizer.requestAuthorization { continuation.resume(returning: $0) }
    }
    guard speech == .authorized else {
      return .denied("Allow speech recognition for Soyeht in System Settings to dictate.")
    }
    guard await Self.microphoneAccess() else {
      return .denied("Allow the microphone for Soyeht in System Settings to dictate.")
    }
    guard let recognizer = SFSpeechRecognizer(locale: locale), recognizer.isAvailable else {
      return .unavailable("Dictation is not available for this language right now.")
    }
    return .authorized
  }

  public func start(onEvent: @escaping @MainActor (SpeechEvent) -> Void) throws {
    cancel()
    guard let recognizer = SFSpeechRecognizer(locale: locale) else {
      throw DictationError.unavailable
    }
    let request = SFSpeechAudioBufferRecognitionRequest()
    request.shouldReportPartialResults = true
    if recognizer.supportsOnDeviceRecognition { request.requiresOnDeviceRecognition = true }
    #if os(iOS)
      let session = AVAudioSession.sharedInstance()
      try session.setCategory(.record, mode: .measurement, options: .duckOthers)
      try session.setActive(true, options: .notifyOthersOnDeactivation)
    #endif
    let engine = AVAudioEngine()
    let input = engine.inputNode
    let format = input.outputFormat(forBus: 0)
    input.installTap(onBus: 0, bufferSize: 1_024, format: format, block: Self.feed(request))
    engine.prepare()
    try engine.start()
    self.engine = engine
    self.request = request
    task = recognizer.recognitionTask(with: request, resultHandler: Self.report(to: onEvent))
  }

  /// The audio tap runs on the audio thread, so it is built outside the main actor.
  private nonisolated static func feed(
    _ request: SFSpeechAudioBufferRecognitionRequest
  ) -> AVAudioNodeTapBlock {
    { buffer, _ in request.append(buffer) }
  }

  /// Results arrive on the recognizer's queue and hop to the main actor.
  private nonisolated static func report(
    to onEvent: @escaping @MainActor (SpeechEvent) -> Void
  ) -> @Sendable (SFSpeechRecognitionResult?, (any Error)?) -> Void {
    { result, error in
      let text = result?.bestTranscription.formattedString
      let isFinal = result?.isFinal ?? false
      let failure = error?.localizedDescription
      Task { @MainActor in
        if let text, isFinal {
          onEvent(.final(text))
        } else if let text {
          onEvent(.partial(text))
        } else if let failure {
          onEvent(.failed(failure))
        }
      }
    }
  }

  public func finish() {
    stopAudio()
    request?.endAudio()
  }

  public func cancel() {
    stopAudio()
    task?.cancel()
    task = nil
    request = nil
  }

  private func stopAudio() {
    guard let engine else { return }
    engine.stop()
    engine.inputNode.removeTap(onBus: 0)
    self.engine = nil
    #if os(iOS)
      try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    #endif
  }

  private static func microphoneAccess() async -> Bool {
    #if os(iOS)
      await AVAudioApplication.requestRecordPermission()
    #else
      await AVCaptureDevice.requestAccess(for: .audio)
    #endif
  }
}

enum DictationError: LocalizedError {
  case unavailable

  var errorDescription: String? { "Dictation is not available on this device." }
}
