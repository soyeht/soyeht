@preconcurrency import AVFoundation
import Foundation
import SoyehtProtocol

/// The audio a voice loop runs on: the microphone, with the system's echo cancellation, and the
/// speaker. The controller uses this interface; tests use a fake.
@MainActor public protocol VoiceAudio: AnyObject {
  /// Called, once, when the audio stops by itself: a device was unplugged, a call interrupted it,
  /// the system reset its audio services. The loop ends with the reason.
  var onFailure: (@MainActor (String) -> Void)? { get set }
  /// Starts the microphone. `onSamples` gets mono samples at 16 kHz, in order, from the audio thread.
  func start(onSamples: @escaping @Sendable ([Float]) -> Void) throws
  func stop()
  /// Runs `start`, which opens a recognizer's microphone: the recognizer joins the loop's audio,
  /// beginning at `milliseconds` of the microphone's clock so the first word of a turn is not lost
  /// to the time it took to notice it. Any other capture that opens the microphone meanwhile is
  /// refused.
  func listening(from milliseconds: Int, _ start: () throws -> Void) rethrows
  /// Plays the speech and returns when it has been heard, or when `stopPlayback` cut it short.
  func play(_ speech: SynthesizedSpeech) async throws
  func stopPlayback()
  /// Seconds of the speech being played that have been heard, from the audio clock.
  var playbackPosition: Double { get }
}

enum VoiceAudioError: LocalizedError, Equatable {
  case notRunning
  case unsupportedAudio
  case noInput
  case busy(String)

  var errorDescription: String? {
    switch self {
    case .notRunning: "The microphone is not running."
    case .unsupportedAudio: "The speech could not be played."
    case .noInput: "There is no microphone to listen with."
    case .busy(let reason): reason
    }
  }
}

/// One audio engine that listens and speaks together, so the system's voice processing can take the
/// speaker out of what the microphone hears: that is what lets a person talk over the avatar
/// through the loudspeaker.
///
/// The microphone has one owner at a time. While this session runs, only the recognizers the voice
/// loop starts (inside `listening(from:_:)`) join it; a dictation that opens the microphone is
/// refused with a message, a loop cannot start while a dictation holds the microphone, and only one
/// loop runs in a process. A loop's recognizers are cancelled by the loop before it stops this session.
///
/// The microphone tap and the player's completion run on audio threads: they are built outside the
/// main actor and never inherit it.
@MainActor public final class VoiceAudioSession: VoiceAudio {
  /// The session recognizers join, while one runs.
  public private(set) static weak var active: VoiceAudioSession?

  public var onFailure: (@MainActor (String) -> Void)?
  private let engine = AVAudioEngine()
  private let player = AVAudioPlayerNode()
  private let input = InputFanOut()
  private var running = false
  private var playerAttached = false
  private var tapInstalled = false
  private var joinAllowed = false
  private var playerRate = 0.0
  private var playback: Playback?
  private var observers: [any NSObjectProtocol] = []

  public init() {}

  public func start(onSamples: @escaping @Sendable ([Float]) -> Void) throws {
    stop()
    guard MicrophoneCapture.ownedCaptures == 0 else {
      throw VoiceAudioError.busy("A dictation is using the microphone. Stop it to start talking.")
    }
    if let other = Self.active, other !== self {
      throw VoiceAudioError.busy("Another conversation is using the microphone.")
    }
    do {
      try configure(onSamples)
    } catch {
      tearDown()
      throw error
    }
  }

  private func configure(_ onSamples: @escaping @Sendable ([Float]) -> Void) throws {
    #if os(iOS)
      // A conversation takes the audio the way a call does: other apps' audio stops while it runs,
      // and the loudspeaker is the default output.
      let session = AVAudioSession.sharedInstance()
      try session.setCategory(
        .playAndRecord, mode: .voiceChat, options: [.defaultToSpeaker, .allowBluetooth])
      try session.setActive(true)
    #endif
    let node = engine.inputNode
    try node.setVoiceProcessingEnabled(true)
    // On macOS the echo canceller also quiets the other audio of the Mac while it runs; the
    // lightest setting keeps it audible.
    node.voiceProcessingOtherAudioDuckingConfiguration = .init(
      enableAdvancedDucking: false, duckingLevel: .min)
    let format = node.outputFormat(forBus: 0)
    guard format.sampleRate > 0, format.channelCount > 0 else { throw VoiceAudioError.noInput }
    engine.attach(player)
    playerAttached = true
    _ = engine.mainMixerNode
    let resampler = try AudioResampler(sampleRate: Double(VoiceFrames.sampleRate))
    input.attach(Self.resampling(resampler, to: onSamples))
    node.installTap(
      onBus: 0, bufferSize: MicrophoneCapture.tapBufferFrames, format: format,
      block: Self.tap(input))
    tapInstalled = true
    engine.prepare()
    try engine.start()
    running = true
    Self.active = self
    observe()
  }

  public func stop() { tearDown() }

  private func tearDown() {
    for observer in observers { NotificationCenter.default.removeObserver(observer) }
    observers = []
    stopPlayback()
    if tapInstalled { engine.inputNode.removeTap(onBus: 0) }
    tapInstalled = false
    engine.stop()
    if playerAttached { engine.detach(player) }
    playerAttached = false
    input.reset()
    playerRate = 0
    joinAllowed = false
    let wasRunning = running
    running = false
    if Self.active === self { Self.active = nil }
    #if os(iOS)
      if wasRunning {
        _ = ClientDiagnostics.attempt("voice_audio_session_release_failed", key: "voice") {
          try AVAudioSession.sharedInstance().setActive(
            false, options: .notifyOthersOnDeactivation)
        }
      }
    #endif
  }

  /// The audio stopped under the loop: tell it why, once.
  private func fail(_ reason: String) {
    guard running else { return }
    tearDown()
    onFailure?(reason)
  }

  private func observe() {
    var names: [Notification.Name] = [.AVAudioEngineConfigurationChange]
    #if os(iOS)
      names += [
        AVAudioSession.interruptionNotification, AVAudioSession.routeChangeNotification,
        AVAudioSession.mediaServicesWereResetNotification,
      ]
    #endif
    observers = names.map { name in
      NotificationCenter.default.addObserver(forName: name, object: nil, queue: nil) {
        [weak self] note in
        guard let reason = Self.failure(of: note) else { return }
        Task { @MainActor in self?.fail(reason) }
      }
    }
  }

  /// What a system audio notification means for the loop: nil when it changes nothing.
  nonisolated private static func failure(of note: Notification) -> String? {
    #if os(iOS)
      if note.name == AVAudioSession.interruptionNotification {
        let kind = note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt
        guard kind == AVAudioSession.InterruptionType.began.rawValue else { return nil }
        return "The conversation was interrupted by another app or a call."
      }
      if note.name == AVAudioSession.routeChangeNotification {
        return "The audio device changed. Start the conversation again."
      }
      if note.name == AVAudioSession.mediaServicesWereResetNotification {
        return "The system reset its audio. Start the conversation again."
      }
    #endif
    return "The audio device changed. Start the conversation again."
  }

  public func listening(from milliseconds: Int, _ start: () throws -> Void) rethrows {
    input.replayFrom = milliseconds
    joinAllowed = true
    defer {
      joinAllowed = false
      input.replayFrom = nil
    }
    try start()
  }

  /// Whether a capture may join now: only a recognizer the loop is starting.
  var allowsJoin: Bool { joinAllowed }

  /// A recognizer's microphone: the audio from the replay point on, then everything live.
  func attach(_ sink: @escaping MicrophoneCapture.Sink) -> Int { input.attach(sink) }

  func detach(_ token: Int) { input.detach(token) }

  public var playbackPosition: Double {
    guard playback != nil, let time = player.lastRenderTime,
      let played = player.playerTime(forNodeTime: time), played.sampleRate > 0
    else { return 0 }
    return max(0, Double(played.sampleTime) / played.sampleRate)
  }

  public func play(_ speech: SynthesizedSpeech) async throws {
    guard running, engine.isRunning else { throw VoiceAudioError.notRunning }
    guard
      let format = AVAudioFormat(
        commonFormat: .pcmFormatFloat32, sampleRate: Double(speech.sampleRate), channels: 1,
        interleaved: false),
      let buffer = AVAudioPCMBuffer(
        pcmFormat: format, frameCapacity: AVAudioFrameCount(speech.samples.count)),
      let channel = buffer.floatChannelData?[0]
    else { throw VoiceAudioError.unsupportedAudio }
    speech.samples.withUnsafeBufferPointer {
      channel.update(from: $0.baseAddress!, count: $0.count)
    }
    buffer.frameLength = AVAudioFrameCount(speech.samples.count)
    stopPlayback()
    if playerRate != format.sampleRate {
      engine.disconnectNodeOutput(player)
      engine.connect(player, to: engine.mainMixerNode, format: format)
      playerRate = format.sampleRate
    }
    let playback = Playback()
    self.playback = playback
    await withTaskCancellationHandler {
      await withCheckedContinuation { continuation in
        playback.begin(continuation)
        player.scheduleBuffer(
          buffer, completionCallbackType: .dataPlayedBack, completionHandler: Self.done(playback))
        player.play()
      }
    } onCancel: {
      playback.finish()
      Task { @MainActor in self.player.stop() }
    }
    if self.playback === playback { self.playback = nil }
  }

  public func stopPlayback() {
    playback?.finish()
    playback = nil
    if playerAttached { player.stop() }
  }

  /// Built outside the main actor: the tap block runs on the audio thread.
  nonisolated private static func tap(_ input: InputFanOut) -> AVAudioNodeTapBlock {
    { buffer, _ in input.deliver(buffer) }
  }

  nonisolated private static func done(_ playback: Playback)
    -> AVAudioPlayerNodeCompletionHandler
  {
    { _ in playback.finish() }
  }

  /// The loop's own sink: the buffers as 16 kHz samples. A buffer that cannot be converted is
  /// reported once and skipped.
  nonisolated private static func resampling(
    _ resampler: AudioResampler, to onSamples: @escaping @Sendable ([Float]) -> Void
  ) -> MicrophoneCapture.Sink {
    { buffer in
      do {
        onSamples(try resampler.samples(from: buffer))
      } catch {
        ClientDiagnostics.reportOnce(
          "voice_audio_resample_failed", key: "voice", error: error)
      }
    }
  }

  /// Resumes the one caller waiting for a speech to end, once, from whichever side ends it first.
  private final class Playback: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Void, Never>?
    private var finished = false

    func begin(_ continuation: CheckedContinuation<Void, Never>) {
      let ended = lock.withLock {
        if !finished { self.continuation = continuation }
        return finished
      }
      if ended { continuation.resume() }
    }

    func finish() {
      let pending = lock.withLock {
        finished = true
        defer { continuation = nil }
        return continuation
      }
      pending?.resume()
    }
  }

  /// Hands each microphone buffer to every sink, and keeps the last two seconds so a sink that
  /// joins late can start from the past. The buffers are copied: the engine reuses its own.
  private final class InputFanOut: @unchecked Sendable {
    private struct Heard {
      let start: Double
      let buffer: AVAudioPCMBuffer
      var end: Double { start + Double(buffer.frameLength) * 1_000 / buffer.format.sampleRate }
    }

    private static let keptMs = Double(ClientPolicy.voiceHistoryMs)
    private let lock = NSLock()
    private var sinks: [Int: MicrophoneCapture.Sink] = [:]
    private var next = 0
    private var heard: [Heard] = []
    private var elapsed = 0.0
    private var replayFromMs: Int?

    var replayFrom: Int? {
      get { lock.withLock { replayFromMs } }
      set { lock.withLock { replayFromMs = newValue } }
    }

    func attach(_ sink: @escaping MicrophoneCapture.Sink) -> Int {
      lock.withLock {
        defer { next += 1 }
        if let from = replayFromMs {
          for past in heard where past.end > Double(from) { sink(past.buffer) }
          replayFromMs = nil
        }
        sinks[next] = sink
        return next
      }
    }

    func detach(_ token: Int) { lock.withLock { _ = sinks.removeValue(forKey: token) } }

    func reset() {
      lock.withLock {
        sinks = [:]
        heard = []
        elapsed = 0
        replayFromMs = nil
      }
    }

    func deliver(_ buffer: AVAudioPCMBuffer) {
      guard
        let copy = AVAudioPCMBuffer(pcmFormat: buffer.format, frameCapacity: buffer.frameLength),
        let source = buffer.floatChannelData, let target = copy.floatChannelData
      else { return }
      copy.frameLength = buffer.frameLength
      for channel in 0..<Int(buffer.format.channelCount) {
        target[channel].update(from: source[channel], count: Int(buffer.frameLength))
      }
      lock.withLock {
        let entry = Heard(start: elapsed, buffer: copy)
        elapsed = entry.end
        heard.append(entry)
        heard.removeAll { $0.end < elapsed - Self.keptMs }
        for sink in sinks.values { sink(copy) }
      }
    }
  }
}
