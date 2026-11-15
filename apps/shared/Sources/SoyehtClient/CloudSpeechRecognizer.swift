import AVFoundation
import Foundation

/// The microphone as the cloud recognizer needs it: buffers to a sink until it stops.
@MainActor protocol AudioCapturing: AnyObject {
  func start(_ sink: @escaping MicrophoneCapture.Sink) throws
  func stop()
}

extension MicrophoneCapture: AudioCapturing {}

/// Dictation through a cloud provider: the microphone fills a recording, and when the person
/// stops, the whole recording goes to `/audio/transcriptions` and the answer is the final
/// transcript. There are no partial transcripts: the endpoint answers once.
@MainActor final class CloudSpeechRecognizer: SpeechRecognizing {
  /// A recording longer than this is cut: the endpoints refuse files over 25 MB, which is more
  /// than ten minutes of 16 kHz mono audio.
  static let maximumSeconds = 600

  private let requests: CloudSpeechRequests
  private let capture: any AudioCapturing
  private let microphone: @MainActor () async -> DictationAuthorization
  private var recording: Recording?
  private var options = SpeechSessionOptions()
  private var onEvent: (@MainActor (SpeechEvent) -> Void)?
  private var sending: Task<Void, Never>?

  init(
    requests: CloudSpeechRequests, capture: any AudioCapturing = MicrophoneCapture(),
    microphone: @escaping @MainActor () async -> DictationAuthorization = {
      await MicrophoneCapture.authorization()
    }
  ) {
    self.requests = requests
    self.capture = capture
    self.microphone = microphone
  }

  func authorize() async -> DictationAuthorization {
    if let reason = unavailableReason() { return .unavailable(reason) }
    return await microphone()
  }

  func start(
    _ options: SpeechSessionOptions, onEvent: @escaping @MainActor (SpeechEvent) -> Void
  ) throws {
    cancel()
    if let reason = unavailableReason() { throw CloudSpeechError.invalidConfiguration(reason) }
    let recording = Recording(limit: WaveAudio.recordingSampleRate * Self.maximumSeconds)
    let resampler = try AudioResampler(sampleRate: Double(WaveAudio.recordingSampleRate))
    try capture.start(Self.sink(resampler, recording))
    self.recording = recording
    self.options = options
    self.onEvent = onEvent
  }

  func finish() {
    capture.stop()
    guard let recording, let onEvent else { return }
    self.recording = nil
    self.onEvent = nil
    let requests = requests
    let options = options
    sending = Task {
      do {
        if let failure = recording.failure { throw failure }
        let text = try await requests.transcribe(
          recording.samples, language: options.language, vocabulary: options.vocabulary)
        if !Task.isCancelled { onEvent(.final(text)) }
      } catch {
        if !Task.isCancelled { onEvent(.failed(error.localizedDescription)) }
      }
    }
  }

  func cancel() {
    capture.stop()
    recording = nil
    onEvent = nil
    sending?.cancel()
    sending = nil
  }

  /// Why the provider cannot be used now: a configuration that is wrong, or a key not saved.
  private func unavailableReason() -> String? {
    if let problem = requests.configuration.problem { return problem }
    guard requests.configuration.requiresKey else { return nil }
    do {
      return try requests.vault.key(for: requests.configuration.id) == nil
        ? CloudSpeechError.missingKey.localizedDescription : nil
    } catch {
      return error.localizedDescription
    }
  }

  /// Runs on the audio thread, so it is built outside the main actor.
  nonisolated private static func sink(
    _ resampler: AudioResampler, _ recording: Recording
  ) -> MicrophoneCapture.Sink {
    { buffer in
      do {
        recording.append(try resampler.samples(from: buffer))
      } catch {
        recording.fail(error)
      }
    }
  }

  /// The samples heard so far, appended on the audio thread and read once the person stops.
  final class Recording: @unchecked Sendable {
    private let lock = NSLock()
    private let limit: Int
    private var heard: [Float] = []
    private var error: (any Error)?

    init(limit: Int) { self.limit = limit }

    var samples: [Float] { lock.withLock { heard } }
    var failure: (any Error)? { lock.withLock { error } }

    func append(_ samples: [Float]) {
      lock.withLock { heard.append(contentsOf: samples.prefix(max(0, limit - heard.count))) }
    }

    func fail(_ failure: any Error) {
      lock.withLock { error = error ?? failure }
    }
  }
}
