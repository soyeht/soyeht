import Foundation
import Observation
import SoyehtProtocol

/// What the person said, and the part of the avatar's last reply they heard if they talked over it.
public struct VoiceTurn: Equatable, Sendable {
  public let text: String
  /// The words of the reply that were played before the person interrupted, or nil when they did
  /// not interrupt a reply. The pane decides how the agent is told.
  public let heardOfInterruptedReply: String?

  public init(text: String, heardOfInterruptedReply: String? = nil) {
    self.text = text
    self.heardOfInterruptedReply = heardOfInterruptedReply
  }
}

public struct VoiceLoopOptions: Sendable {
  public var turn = VoiceTurnConfig()
  /// The language the person speaks, as the recognizer takes it, or nil for the recognizer's own.
  public var language: String?
  public var vocabulary: [String] = []
  public var voice = SpeechSynthesisOptions()
  public var sentences = SentenceChunker()
  /// The loop ends with a message when the microphone delivers no audio for this long.
  public var watchdog = Duration.milliseconds(ClientPolicy.voiceWatchdogMs)

  public init() {}
}

public enum VoiceLoopError: LocalizedError, Equatable {
  case notAuthorized(String)

  public var errorDescription: String? {
    switch self {
    case .notAuthorized(let reason): reason
    }
  }
}

/// The voice loop of an avatar pane, in the client that has the microphone (ADR 0166 section 5):
/// the microphone, the voice-activity and end-of-turn models and the turn machine decide when the
/// person has spoken; a recognizer turns the audio into text that goes out as a `VoiceTurn`; the
/// agent's replies come back as text, are cut into sentences, spoken one by one, and can be talked over.
///
/// The controller holds no conversation rules of its own: when to listen, speak or give way is
/// `VoiceTurnMachine`, and what the agent says and how is the agent's. The pane drives it: `start`,
/// `replyUpdated` for each growth of a message of the avatar (with the message's identity, so late
/// words of a cancelled reply are never spoken), `replyEnded` when a turn's reply is over with
/// nothing to say, and the two callbacks. A reply is spoken whether or not a voice turn preceded it.
///
/// The microphone has one owner (`VoiceAudioSession`): a dictation cannot run while the loop does.
///
/// For the stage: `state`, `phase`, `inputLevel`, `outputLevel`, and `currentSpeech` with
/// `playbackPosition`, the audio and timings of the sentence being heard.
@MainActor @Observable public final class VoiceLoopController {
  public private(set) var state = VoiceTurnState.idle
  public private(set) var phase = VoiceTurnPhase.off
  /// What the recognizer has heard of the person's turn so far.
  public private(set) var partialTranscript = ""
  /// The microphone's loudness, 0 to 1, per frame.
  public private(set) var inputLevel: Float = 0
  /// The loudness of what is playing, 0 to 1, about every 16 ms while the avatar speaks.
  public private(set) var outputLevel: Float = 0
  /// The sentence being heard, with the audio and the timings the voice reported.
  public private(set) var currentSpeech: SynthesizedSpeech?
  public private(set) var currentSentence: String?
  /// Why the last thing failed (a recognizer, a voice, a model, the audio), until the next turn
  /// starts. A failure of the audio ends the loop.
  public private(set) var failure: String?

  /// The person finished a turn: send it to the avatar's agent.
  @ObservationIgnored public var onTurn: (@MainActor (VoiceTurn) -> Void)?
  /// The agent's reply is no longer wanted (the person spoke, or stopped it): cancel it.
  @ObservationIgnored public var onCancelReply: (@MainActor () -> Void)?

  @ObservationIgnored let audio: any VoiceAudio
  @ObservationIgnored private let makeRecognizer: @MainActor () -> any SpeechRecognizing
  @ObservationIgnored private let synthesizer: any SpeechSynthesizing
  @ObservationIgnored private let frontEnd: VoiceFrontEnd
  @ObservationIgnored private let endOfTurn: any EndOfTurnDetecting
  @ObservationIgnored private let options: VoiceLoopOptions
  @ObservationIgnored private let cleanup: Cleanup

  @ObservationIgnored private var machine: VoiceTurnMachine
  @ObservationIgnored private var startToken = 0
  @ObservationIgnored private var feeding: Task<Void, Never>?
  @ObservationIgnored private var watching: Task<Void, Never>?
  @ObservationIgnored private var lastAudio = ContinuousClock.now
  @ObservationIgnored private var clock = 0

  @ObservationIgnored private var recognizer: (any SpeechRecognizing)?
  @ObservationIgnored private var recognition = 0
  @ObservationIgnored private var lastPartial = ""
  @ObservationIgnored private var endOfTurnCheck: Task<Void, Never>?

  @ObservationIgnored private var replies: ReplyFeed
  @ObservationIgnored private var sentences: [String] = []
  @ObservationIgnored private var played: [String] = []
  /// A turn was sent and its reply has not begun: the loop is still thinking, not done.
  @ObservationIgnored private var awaitingReply = false
  @ObservationIgnored private var speaking: Task<Void, Never>?
  /// Counts the times speaking was cut off, so a task that was cancelled does not undo its successor.
  @ObservationIgnored private var speechEpoch = 0
  @ObservationIgnored private var synthesizing: [Task<SynthesizedSpeech, any Error>] = []
  @ObservationIgnored private var ticker: Task<Void, Never>?
  @ObservationIgnored private var heardOnInterruption: String?

  public init(
    audio: any VoiceAudio, recognizer: @escaping @MainActor () -> any SpeechRecognizing,
    synthesizer: any SpeechSynthesizing, activity: any VoiceActivityDetecting,
    endOfTurn: any EndOfTurnDetecting = SilenceEndOfTurn(),
    options: VoiceLoopOptions = VoiceLoopOptions()
  ) {
    self.audio = audio
    makeRecognizer = recognizer
    self.synthesizer = synthesizer
    frontEnd = VoiceFrontEnd(detector: activity)
    self.endOfTurn = endOfTurn
    self.options = options
    machine = VoiceTurnMachine(options.turn)
    replies = ReplyFeed(sentences: options.sentences)
    cleanup = Cleanup(audio)
    audio.onFailure = { [weak self] reason in self?.audioFailed(reason) }
  }

  /// Stops the audio of a controller that is dropped while it runs.
  private final class Cleanup: Sendable {
    nonisolated(unsafe) private let audio: any VoiceAudio

    init(_ audio: any VoiceAudio) { self.audio = audio }

    deinit {
      let audio = audio
      Task { @MainActor in audio.stop() }
    }
  }

  /// The audio clock of the microphone, in milliseconds.
  public var now: Int { clock }

  /// Seconds of the current sentence that have been heard.
  public var playbackPosition: Double { audio.playbackPosition }

  // MARK: Lifecycle

  /// Asks for the microphone and speech recognition, then listens. Throws when the person refuses
  /// or the microphone is not available. A `stop` or a newer `start` that comes while the person is
  /// answering the permission prompt cancels this one.
  public func start() async throws {
    guard phase == .off else { return }
    startToken += 1
    let token = startToken
    switch await makeRecognizer().authorize() {
    case .authorized: break
    case .denied(let reason), .unavailable(let reason): throw VoiceLoopError.notAuthorized(reason)
    }
    guard token == startToken, phase == .off else { return }
    await frontEnd.reset()
    guard token == startToken, phase == .off else { return }
    let (samples, handoff) = AsyncStream.makeStream(of: [Float].self)
    try audio.start { handoff.yield($0) }
    clock = 0
    failure = nil
    lastAudio = .now
    feeding?.cancel()
    feeding = Task { [weak self] in
      for await chunk in samples {
        guard let self else { return }
        await self.feed(chunk)
      }
    }
    watching?.cancel()
    watching = Task { [weak self, watchdog = options.watchdog] in
      while !Task.isCancelled {
        do { try await Task.sleep(for: watchdog / 3) } catch { return }
        guard let self else { return }
        self.checkAudioFlows(within: watchdog)
      }
    }
    send(.start)
  }

  public func stop() {
    startToken += 1
    send(.stop)
    feeding?.cancel()
    feeding = nil
    watching?.cancel()
    watching = nil
    endOfTurnCheck?.cancel()
    endOfTurnCheck = nil
    cancelRecognition()
    cutSpeaking()
    sentences = []
    played = []
    awaitingReply = false
    audio.stop()
  }

  /// The person pressed the key or clicked to stop the avatar, or to drop what they were saying.
  public func interrupt() { send(.interrupt) }

  private func audioFailed(_ reason: String) {
    stop()
    failure = reason
  }

  private func checkAudioFlows(within limit: Duration) {
    guard phase != .off, ContinuousClock.now - lastAudio > limit else { return }
    audioFailed("The microphone stopped delivering audio.")
  }

  // MARK: Microphone

  /// Microphone samples, 16 kHz mono, in order: framed and read off the main actor, then given
  /// to the turn machine.
  func feed(_ samples: [Float]) async {
    let result = await frontEnd.process(samples)
    guard phase != .off else { return }
    lastAudio = .now
    if let reason = result.failure { failure = reason }
    for frame in result.frames {
      clock = frame.at
      inputLevel = frame.level
      send(.frame(probability: frame.probability))
    }
  }

  private func send(_ input: VoiceTurnInput) {
    for action in machine.handle(input, at: clock) { perform(action) }
    state = machine.state
    phase = machine.phase
    pump()
  }

  private func perform(_ action: VoiceTurnAction) {
    switch action {
    case .startTurn(let from): beginRecognition(from: from, probe: false)
    case .probeInterruption(let from): beginRecognition(from: from, probe: true)
    case .checkEndOfTurn(let id): checkEndOfTurn(id)
    case .finalizeTurn, .finishProbe: finishRecognizer()
    case .discardTurn, .dropProbe: cancelRecognition()
    case .submit(let text, let interrupted): submit(text, interrupted: interrupted)
    case .cancelReply: cancelReply()
    case .stopPlayback: stopPlayback()
    }
  }

  // MARK: The person's turn

  private func beginRecognition(from milliseconds: Int, probe: Bool) {
    cancelRecognition()
    lastPartial = ""
    if !probe {
      partialTranscript = ""
      failure = nil
    }
    let id = recognition
    let recognizer = makeRecognizer()
    let session = SpeechSessionOptions(language: options.language, vocabulary: options.vocabulary)
    do {
      try audio.listening(from: milliseconds) {
        try recognizer.start(session) { [weak self] event in self?.recognized(event, of: id) }
      }
      self.recognizer = recognizer
    } catch {
      failure = error.localizedDescription
    }
  }

  private func cancelRecognition() {
    recognizer?.cancel()
    recognizer = nil
    recognition += 1
  }

  /// The turn, or the probe, is over: the recognizer stops listening and gives its final text. When
  /// it has none to give, the text it had is the final one.
  private func finishRecognizer() {
    guard let recognizer else {
      send(.transcript(lastPartial, final: true))
      return
    }
    recognizer.finish()
  }

  private func recognized(_ event: SpeechEvent, of id: Int) {
    guard id == recognition else { return }
    switch event {
    case .partial(let text): hear(text, final: false)
    case .final(let text): hear(text, final: true)
    case .failed(let reason):
      failure = reason
      hear(lastPartial, final: true)
    }
  }

  /// What the recognizer heard. While it listens to someone talking over the avatar, that is mostly
  /// the avatar's own voice, so it is shown only once it has taken the turn.
  private func hear(_ text: String, final: Bool) {
    lastPartial = text
    if final {
      recognizer = nil
      recognition += 1
    }
    let probing = machine.phase == .probing || machine.phase == .probingWait
    if !probing { partialTranscript = text }
    send(.transcript(text, final: final))
    if probing, machine.phase == .hearing { partialTranscript = text }
  }

  private func checkEndOfTurn(_ id: Int) {
    endOfTurnCheck?.cancel()
    endOfTurnCheck = Task { [weak self, endOfTurn, frontEnd] in
      let window = await frontEnd.window()
      var complete = true
      var problem: String?
      do {
        complete = try await endOfTurn.isComplete(turn: window)
      } catch {
        problem = error.localizedDescription
      }
      guard !Task.isCancelled, let self else { return }
      if let problem { self.failure = problem }
      self.send(.endOfTurn(id: id, complete: complete))
    }
  }

  private func submit(_ text: String, interrupted: Bool) {
    let heard = interrupted ? heardOnInterruption : nil
    heardOnInterruption = nil
    played = []
    awaitingReply = true
    onTurn?(VoiceTurn(text: text, heardOfInterruptedReply: heard))
  }

  // MARK: The avatar's replies

  /// The message `id` of the avatar has grown to `markdown`, whole when `final`. Its sentences are
  /// spoken once the person is not talking; nothing is spoken for a message that was cancelled.
  public func replyUpdated(_ markdown: String, final: Bool, id: String) {
    guard phase != .off else { return }
    awaitingReply = false
    sentences += replies.update(id: id, markdown: markdown, final: final)
    pump()
  }

  /// The reply to the person's turn ended with nothing to say (it failed, or was only code).
  public func replyEnded() {
    awaitingReply = false
    pump()
  }

  /// Speaks what waits when the person is not talking, and ends the reply when it is all said.
  private func pump() {
    guard speaking == nil, phase != .off else { return }
    let current = machine.phase
    if !sentences.isEmpty {
      guard [.waiting, .waitingReply, .playing].contains(current) else { return }
      let epoch = speechEpoch
      speaking = Task { [weak self] in
        await self?.speakQueue()
        guard let self, self.speechEpoch == epoch else { return }
        self.speaking = nil
        self.pump()
      }
    } else if !awaitingReply, !replies.hasPending,
      [.waitingReply, .playing, .probing, .probingWait].contains(current)
    {
      played = []
      send(.replyFinished)
    }
  }

  /// Speaks the queue in order, the voice already working on the next sentence while one plays.
  private func speakQueue() async {
    var upcoming: (sentence: String, speech: Task<SynthesizedSpeech, any Error>)?
    while !Task.isCancelled, let (sentence, speech) = take(upcoming) {
      upcoming = sentences.first.map { ($0, synthesis(of: $0)) }
      do {
        let heard = try await speech.value
        try Task.checkCancellation()
        try await play(heard, saying: sentence)
        played.append(sentence)
      } catch is CancellationError {
        return
      } catch {
        failure = error.localizedDescription
      }
    }
  }

  /// The next sentence to speak with its audio on the way: the one already started, or the first
  /// that waits.
  private func take(
    _ started: (sentence: String, speech: Task<SynthesizedSpeech, any Error>)?
  ) -> (String, Task<SynthesizedSpeech, any Error>)? {
    if let started, !sentences.isEmpty {
      sentences.removeFirst()
      return started
    }
    guard let first = sentences.first else { return nil }
    sentences.removeFirst()
    return (first, synthesis(of: first))
  }

  private func synthesis(of sentence: String) -> Task<SynthesizedSpeech, any Error> {
    let task = Task { [synthesizer, voice = options.voice] in
      try await synthesizer.synthesize(sentence, options: voice)
    }
    synthesizing.append(task)
    return task
  }

  private func play(_ speech: SynthesizedSpeech, saying sentence: String) async throws {
    currentSpeech = speech
    currentSentence = sentence
    send(.sentenceStarted(sentence))
    ticker = Task { [weak self] in
      while !Task.isCancelled {
        self?.updateOutputLevel()
        do { try await Task.sleep(for: .milliseconds(ClientPolicy.voiceOutputTickMs)) } catch {
          return
        }
      }
    }
    defer {
      ticker?.cancel()
      ticker = nil
      outputLevel = 0
      currentSpeech = nil
      currentSentence = nil
    }
    try await audio.play(speech)
    try Task.checkCancellation()
  }

  private func updateOutputLevel() {
    guard let speech = currentSpeech else { return }
    outputLevel = SpeechEnvelope.level(
      of: speech.samples, sampleRate: speech.sampleRate, at: audio.playbackPosition)
  }

  private func stopPlayback() {
    heardOnInterruption = heardNow()
    cutSpeaking()
    audio.stopPlayback()
  }

  private func cancelReply() {
    cutSpeaking()
    replies.cancel()
    sentences = []
    played = []
    awaitingReply = false
    onCancelReply?()
  }

  private func cutSpeaking() {
    speechEpoch += 1
    speaking?.cancel()
    speaking = nil
    for task in synthesizing { task.cancel() }
    synthesizing = []
  }

  /// What has been heard of the reply: the sentences played, and the part of the one playing that
  /// its position in the audio covers.
  private func heardNow() -> String {
    let seconds =
      currentSpeech.map { Double($0.samples.count) / Double(max($0.sampleRate, 1)) } ?? 0
    let position = audio.playbackPosition
    return HeardSpeech.text(
      played: played, current: currentSentence, fraction: seconds > 0 ? position / seconds : 0,
      timings: currentSpeech?.timings, position: position)
  }
}
