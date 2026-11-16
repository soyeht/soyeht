@preconcurrency import AVFoundation
import Foundation

/// The audio a voice loop runs on: the microphone, with the system's echo cancellation, and the
/// speaker. The controller uses this interface; tests use a fake.
@MainActor public protocol VoiceAudio: AnyObject {
  /// Starts the microphone. `onSamples` gets mono samples at 16 kHz, in order, from the audio thread.
  func start(onSamples: @escaping @Sendable ([Float]) -> Void) throws
  func stop()
  /// The next recognizer that opens the microphone gets its audio from `milliseconds` of the
  /// microphone's clock on, so the first word of a turn is not lost to the time it took to notice it.
  func replay(from milliseconds: Int)
  /// Plays the speech and returns when it has been heard, or when `stopPlayback` cut it short.
  func play(_ speech: SynthesizedSpeech) async throws
  func stopPlayback()
  /// Seconds of the speech being played that have been heard, from the audio clock.
  var playbackPosition: Double { get }
}

enum VoiceAudioError: LocalizedError {
  case notRunning
  case unsupportedAudio

  var errorDescription: String? {
    switch self {
    case .notRunning: "The microphone is not running."
    case .unsupportedAudio: "The speech could not be played."
    }
  }
}

/// One audio engine that listens and speaks together, so the system's voice processing can take the
/// speaker out of what the microphone hears: that is what lets a person talk over the avatar
/// through the loudspeaker.
///
/// While it runs, a `MicrophoneCapture` (every recognizer's way to the microphone) joins it
/// instead of opening the device: the recognizers are written for one microphone each, and the
/// device has only one echo-cancelled input.
///
/// The microphone tap and the player's completion run on audio threads: they are built outside the
/// main actor and never inherit it.
@MainActor public final class VoiceAudioSession: VoiceAudio {
  /// The session recognizers join, while one runs.
  public private(set) static weak var active: VoiceAudioSession?

  private let engine = AVAudioEngine()
  private let player = AVAudioPlayerNode()
  private let input = InputFanOut()
  private var running = false
  private var playerRate = 0.0
  private var playback: Playback?

  public init() {}

  public func start(onSamples: @escaping @Sendable ([Float]) -> Void) throws {
    stop()
    #if os(iOS)
      let session = AVAudioSession.sharedInstance()
      try session.setCategory(
        .playAndRecord, mode: .voiceChat, options: [.defaultToSpeaker, .allowBluetooth])
      try session.setActive(true)
    #endif
    let node = engine.inputNode
    try node.setVoiceProcessingEnabled(true)
    // The echo canceller ducks other audio by default; the quietest ducking leaves the person's
    // music or video audible.
    node.voiceProcessingOtherAudioDuckingConfiguration = .init(
      enableAdvancedDucking: false, duckingLevel: .min)
    engine.attach(player)
    _ = engine.mainMixerNode
    let resampler = try AudioResampler(sampleRate: Double(VoiceFrames.sampleRate))
    input.attach(Self.resampling(resampler, to: onSamples))
    node.installTap(
      onBus: 0, bufferSize: 1_024, format: node.outputFormat(forBus: 0),
      block: Self.tap(input))
    engine.prepare()
    try engine.start()
    running = true
    Self.active = self
  }

  public func stop() {
    guard running else { return }
    stopPlayback()
    engine.stop()
    engine.inputNode.removeTap(onBus: 0)
    engine.detach(player)
    input.reset()
    running = false
    playerRate = 0
    if Self.active === self { Self.active = nil }
    #if os(iOS)
      _ = ClientDiagnostics.attempt("voice_audio_session_release_failed", key: "voice") {
        try AVAudioSession.sharedInstance().setActive(
          false, options: .notifyOthersOnDeactivation)
      }
    #endif
  }

  public func replay(from milliseconds: Int) { input.replayFrom = milliseconds }

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
    guard running else { throw VoiceAudioError.notRunning }
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
    player.stop()
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

    private static let keptMs = 2_000.0
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
