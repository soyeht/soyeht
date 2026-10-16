import AVFoundation
import Foundation

/// The microphone, as every dictation engine uses it: the permission request and one running
/// audio engine whose buffers go to the engine's sink.
///
/// The sink runs on the realtime audio thread. It is `@Sendable` so it never inherits the main
/// actor from the engine that built it, and it must not touch main-actor state.
@MainActor public final class MicrophoneCapture {
  public typealias Sink = @Sendable (AVAudioPCMBuffer) -> Void

  private var engine: AVAudioEngine?

  public init() {}

  /// Asks for microphone access; the system answers on a background queue.
  public static func requestAccess() async -> Bool {
    #if os(iOS)
      await SystemCallback.value {
        AVAudioApplication.requestRecordPermission(completionHandler: $0)
      }
    #else
      await SystemCallback.value {
        AVCaptureDevice.requestAccess(for: .audio, completionHandler: $0)
      }
    #endif
  }

  /// The permission outcome as the dictation flow reports it.
  public static func authorization() async -> DictationAuthorization {
    await requestAccess()
      ? .authorized
      : .denied("Allow the microphone for Soyeht in System Settings to dictate.")
  }

  /// Starts delivering buffers to `sink`. Replaces a capture that is already running.
  public func start(_ sink: @escaping Sink) throws {
    stop()
    #if os(iOS)
      let session = AVAudioSession.sharedInstance()
      try session.setCategory(.record, mode: .measurement, options: .duckOthers)
      try session.setActive(true, options: .notifyOthersOnDeactivation)
    #endif
    let engine = AVAudioEngine()
    let input = engine.inputNode
    input.installTap(
      onBus: 0, bufferSize: 1_024, format: input.outputFormat(forBus: 0), block: Self.tap(sink))
    engine.prepare()
    try engine.start()
    self.engine = engine
  }

  public func stop() {
    guard let engine else { return }
    engine.stop()
    engine.inputNode.removeTap(onBus: 0)
    self.engine = nil
    #if os(iOS)
      _ = ClientDiagnostics.attempt("dictation_audio_session_release_failed", key: "dictation") {
        try AVAudioSession.sharedInstance().setActive(
          false, options: .notifyOthersOnDeactivation)
      }
    #endif
  }

  /// Built outside the main actor: the tap block runs on the audio thread.
  nonisolated static func tap(_ sink: @escaping Sink) -> AVAudioNodeTapBlock {
    { buffer, _ in sink(buffer) }
  }
}
