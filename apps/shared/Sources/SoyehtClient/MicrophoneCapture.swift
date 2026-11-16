import AVFoundation
import Foundation

/// The microphone, as every dictation engine uses it: the permission request and one running
/// audio engine whose buffers go to the engine's sink.
///
/// The sink runs on the realtime audio thread. It is `@Sendable` so it never inherits the main
/// actor from the engine that built it, and it must not touch main-actor state.
///
/// One engine owns the device. While a voice loop runs, that engine is the loop's echo-cancelled
/// session, and a capture joins it instead of opening the device: one that starts then joins at
/// once, and one that is running on its own engine when the loop starts is moved onto the session.
/// When the loop stops, a capture that it holds is handed back to an engine of its own, or ended
/// with `onEnded` when that fails.
@MainActor public final class MicrophoneCapture {
  public typealias Sink = @Sendable (AVAudioPCMBuffer) -> Void

  /// The frames of one tap buffer, for every capture of the app.
  static let tapBufferFrames: AVAudioFrameCount = 1_024

  private struct Weak {
    weak var capture: MicrophoneCapture?
  }

  /// The captures that hold the device on an engine of their own. They are kept weakly: a capture
  /// that is dropped without `stop()` lets go of the device with its engine.
  private static var live: [Weak] = []

  /// The captures running on an engine of their own now.
  static var ownEngineCaptures: [MicrophoneCapture] {
    live = live.filter { $0.capture != nil }
    return live.compactMap(\.capture).filter { $0.engine != nil }
  }

  private var engine: AVAudioEngine?
  private var sink: Sink?
  /// Set while this capture listens through the voice loop's session instead of its own engine.
  private var joined: (session: VoiceAudioSession, token: Int)?
  /// The capture ended without `stop()`: the loop that held it stopped and no engine of its own
  /// could be opened for it.
  public var onEnded: (@MainActor (any Error) -> Void)?

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
    self.sink = sink
    if let session = VoiceAudioSession.active {
      joined = (session, session.attach(sink, owner: self))
      return
    }
    do {
      try openOwnEngine(sink)
    } catch {
      self.sink = nil
      throw error
    }
  }

  public func stop() {
    sink = nil
    if let joined {
      joined.session.detach(joined.token)
      self.joined = nil
      return
    }
    closeOwnEngine()
    #if os(iOS)
      // The loop's session is its own: a dictation ending must not deactivate it.
      guard VoiceAudioSession.active == nil else { return }
      _ = ClientDiagnostics.attempt("dictation_audio_session_release_failed", key: "dictation") {
        try AVAudioSession.sharedInstance().setActive(
          false, options: .notifyOthersOnDeactivation)
      }
    #endif
  }

  private func openOwnEngine(_ sink: @escaping Sink) throws {
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
    do {
      try engine.start()
    } catch {
      input.removeTap(onBus: 0)
      throw error
    }
    self.engine = engine
    Self.live = Self.live.filter { $0.capture != nil }
    if !Self.live.contains(where: { $0.capture === self }) { Self.live.append(Weak(capture: self)) }
  }

  private func closeOwnEngine() {
    guard let engine else { return }
    engine.stop()
    engine.inputNode.removeTap(onBus: 0)
    self.engine = nil
  }

  /// The voice loop starts: the capture lets go of the device and keeps its sink, which the loop's
  /// session takes (`join`) or gives back (`handBack`).
  func release() -> Sink? {
    closeOwnEngine()
    return sink
  }

  func join(_ session: VoiceAudioSession, sink: @escaping Sink) {
    joined = (session, session.attach(sink, owner: self))
  }

  /// The voice loop stops: an engine of the capture's own again, or an end with the reason.
  func handBack() {
    joined = nil
    guard let sink else { return }
    do {
      try openOwnEngine(sink)
    } catch {
      self.sink = nil
      onEnded?(error)
    }
  }

  /// Built outside the main actor: the tap block runs on the audio thread.
  nonisolated static func tap(_ sink: @escaping Sink) -> AVAudioNodeTapBlock {
    { buffer, _ in sink(buffer) }
  }
}
