import AVFoundation
import Foundation

/// The microphone, as every dictation engine uses it: the permission request and one running
/// audio engine whose buffers go to the engine's sink.
///
/// The sink runs on the realtime audio thread. It is `@Sendable` so it never inherits the main
/// actor from the engine that built it, and it must not touch main-actor state.
@MainActor public final class MicrophoneCapture {
  public typealias Sink = @Sendable (AVAudioPCMBuffer) -> Void

  /// The frames of one tap buffer, for every capture of the app.
  static let tapBufferFrames: AVAudioFrameCount = 1_024
  /// How many captures hold the device on their own engine: the voice loop does not start while a
  /// dictation does.
  private(set) static var ownedCaptures = 0

  private var engine: AVAudioEngine?
  /// Set while this capture listens through the voice loop's session instead of its own engine.
  private var joined: (session: VoiceAudioSession, token: Int)?

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

  /// Starts delivering buffers to `sink`. Replaces a capture that is already running. While a
  /// voice loop runs, the capture joins its echo-cancelled session rather than opening the device.
  public func start(_ sink: @escaping Sink) throws {
    stop()
    if let session = VoiceAudioSession.active {
      // The device is the voice loop's: only the recognizers it starts join it.
      guard session.allowsJoin else {
        throw VoiceAudioError.busy("The microphone is in use by the voice conversation.")
      }
      joined = (session, session.attach(sink))
      return
    }
    #if os(iOS)
      let session = AVAudioSession.sharedInstance()
      try session.setCategory(.record, mode: .measurement, options: .duckOthers)
      try session.setActive(true, options: .notifyOthersOnDeactivation)
    #endif
    let engine = AVAudioEngine()
    let input = engine.inputNode
    input.installTap(
      onBus: 0, bufferSize: Self.tapBufferFrames, format: input.outputFormat(forBus: 0),
      block: Self.tap(sink))
    engine.prepare()
    try engine.start()
    Self.ownedCaptures += 1
    self.engine = engine
  }

  public func stop() {
    if let joined {
      joined.session.detach(joined.token)
      self.joined = nil
      return
    }
    guard let engine else { return }
    engine.stop()
    engine.inputNode.removeTap(onBus: 0)
    self.engine = nil
    Self.ownedCaptures -= 1
    #if os(iOS)
      // The loop's session is its own: a dictation ending must not deactivate it.
      guard VoiceAudioSession.active == nil else { return }
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
