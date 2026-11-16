import Foundation
import Observation

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
  /// How long the recognizer has, after the end of a turn, to give its final text before the last
  /// partial one is used.
  public var finalTranscriptTimeout = Duration.seconds(3)

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
/// agent's reply comes back as text, is cut into sentences, spoken one by one, and can be talked over.
///
/// The controller holds no conversation rules of its own: when to listen, speak or give way is
/// `VoiceTurnMachine`, and what the agent says and how is the agent's. The pane drives it:
/// `start`, `replyUpdated` for each growth of the avatar's message, and the two callbacks.
///
/// For the stage: `state`, `phase`, `inputLevel`, `outputLevel`, and `currentSpeech` with
/// `playbackPosition`, the audio and timings of the sentence being heard.
@MainActor @Observable public final class VoiceLoopController {
  public private(set) var state = VoiceTurnState.idle
  public private(set) var phase = VoiceTurnPhase.off
  /// What the recognizer has heard of the turn so far.
  public private(set) var partialTranscript = ""
  /// The microphone's loudness, 0 to 1, per frame.
  public private(set) var inputLevel: Float = 0
  /// The loudness of what is playing, 0 to 1, about every 16 ms while the avatar speaks.
  public private(set) var outputLevel: Float = 0
  /// The sentence being heard, with the audio and the timings the voice reported.
  public private(set) var currentSpeech: SynthesizedSpeech?
  public private(set) var currentSentence: String?
  /// Why the last thing failed (a recognizer or a voice), until the next turn starts.
  public private(set) var failure: String?

  /// The person finished a turn: send it to the avatar's agent.
  @ObservationIgnored public var onTurn: (@MainActor (VoiceTurn) -> Void)?
  /// The agent's reply is no longer wanted (the person spoke, or stopped it): cancel it.
  @ObservationIgnored public var onCancelReply: (@MainActor () -> Void)?

  @ObservationIgnored private let audio: any VoiceAudio
  @ObservationIgnored private let makeRecognizer: @MainActor () -> any SpeechRecognizing
  @ObservationIgnored private let synthesizer: any SpeechSynthesizing
  @ObservationIgnored private let activity: any VoiceActivityDetecting
  @ObservationIgnored private let endOfTurn: any EndOfTurnDetecting
  @ObservationIgnored private let options: VoiceLoopOptions

  @ObservationIgnored private var machine: VoiceTurnMachine
  @ObservationIgnored private var feeding: Task<Void, Never>?
  @ObservationIgnored private var pending: [Float] = []
  @ObservationIgnored private var turnAudio: [Float] = []
  @ObservationIgnored private var samplesSeen = 0

  @ObservationIgnored private var recognizer: (any SpeechRecognizing)?
  @ObservationIgnored private var recognition = 0
  @ObservationIgnored private var finalizing: Task<Void, Never>?
  @ObservationIgnored private var endOfTurnCheck: Task<Void, Never>?

  @ObservationIgnored private var reply: ReplyQueue
  @ObservationIgnored private var speaking: Task<Void, Never>?
  /// Counts the times speaking was cut off, so a task that was cancelled does not undo its successor.
  @ObservationIgnored private var speechEpoch = 0
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
    self.activity = activity
    self.endOfTurn = endOfTurn
    self.options = options
    machine = VoiceTurnMachine(options.turn)
    reply = ReplyQueue(chunker: options.sentences)
  }

  /// The audio clock of the microphone, in milliseconds.
  public var now: Int { samplesSeen * 1_000 / VoiceFrames.sampleRate }

  /// Seconds of the current sentence that have been heard.
  public var playbackPosition: Double { audio.playbackPosition }

  // MARK: Lifecycle

  /// Asks for the microphone and speech recognition, then listens. Throws when the person refuses.
  public func start() async throws {
    guard phase == .off else { return }
    switch await makeRecognizer().authorize() {
    case .authorized: break
    case .denied(let reason), .unavailable(let reason): throw VoiceLoopError.notAuthorized(reason)
    }
    let (samples, handoff) = AsyncStream.makeStream(of: [Float].self)
    try audio.start { handoff.yield($0) }
    activity.reset()
    samplesSeen = 0
    pending = []
    turnAudio = []
    failure = nil
    feeding = Task { [weak self] in
      for await chunk in samples { self?.feed(chunk) }
    }
    send(.start)
  }

  public func stop() {
    send(.stop)
    feeding?.cancel()
    feeding = nil
    audio.stop()
  }

  /// The person pressed the key or clicked to stop the avatar, or to drop what they were saying.
  public func interrupt() { send(.interrupt) }

  // MARK: Microphone

  /// Microphone samples, 16 kHz mono, in order. Cut into frames, each read by the voice-activity
  /// model and given to the turn machine.
  func feed(_ samples: [Float]) {
    pending += samples
    while pending.count >= VoiceFrames.size {
      let frame = Array(pending.prefix(VoiceFrames.size))
      pending.removeFirst(VoiceFrames.size)
      turnAudio = Array((turnAudio + frame).suffix(VoiceFrames.turnWindow))
      samplesSeen += VoiceFrames.size
      inputLevel = SpeechEnvelope.level(
        of: frame, sampleRate: VoiceFrames.sampleRate, at: 0,
        windowSeconds: Double(VoiceFrames.frameMs) / 1_000)
      send(.frame(probability: activity.probability(of: frame)))
    }
  }

  private func send(_ input: VoiceTurnInput) {
    for action in machine.handle(input, at: now) { perform(action) }
    state = machine.state
    phase = machine.phase
  }

  private func perform(_ action: VoiceTurnAction) {
    switch action {
    case .startTurn(let from), .probeInterruption(let from): beginRecognition(from: from)
    case .checkEndOfTurn(let id): checkEndOfTurn(id)
    case .finalizeTurn: finalizeRecognition()
    case .discardTurn, .dropProbe: cancelRecognition()
    case .submit(let text, let interrupted): submit(text, interrupted: interrupted)
    case .cancelReply: cancelReply()
    case .stopPlayback: stopPlayback()
    }
  }

  // MARK: The person's turn

  private func beginRecognition(from milliseconds: Int) {
    cancelRecognition()
    partialTranscript = ""
    failure = nil
    let id = recognition
    let recognizer = makeRecognizer()
    audio.replay(from: milliseconds)
    do {
      try recognizer.start(
        SpeechSessionOptions(language: options.language, vocabulary: options.vocabulary)
      ) { [weak self] event in self?.recognized(event, of: id) }
      self.recognizer = recognizer
    } catch {
      failure = error.localizedDescription
    }
  }

  private func cancelRecognition() {
    finalizing?.cancel()
    finalizing = nil
    recognizer?.cancel()
    recognizer = nil
    recognition += 1
  }

  private func finalizeRecognition() {
    guard let recognizer else {
      send(.transcript("", final: true))
      return
    }
    recognizer.finish()
    let id = recognition
    finalizing = Task { [weak self, timeout = options.finalTranscriptTimeout] in
      do { try await Task.sleep(for: timeout) } catch { return }
      guard let self, self.recognition == id else { return }
      self.recognizer?.cancel()
      self.finishRecognition(with: self.partialTranscript)
    }
  }

  private func recognized(_ event: SpeechEvent, of id: Int) {
    guard id == recognition else { return }
    switch event {
    case .partial(let text):
      partialTranscript = text
      send(.transcript(text, final: false))
    case .final(let text):
      finishRecognition(with: text)
    case .failed(let reason):
      failure = reason
      finishRecognition(with: partialTranscript)
    }
  }

  private func finishRecognition(with text: String) {
    finalizing?.cancel()
    finalizing = nil
    recognizer = nil
    recognition += 1
    partialTranscript = text
    send(.transcript(text, final: true))
  }

  private func checkEndOfTurn(_ id: Int) {
    endOfTurnCheck?.cancel()
    let window = turnAudio
    endOfTurnCheck = Task { [weak self, endOfTurn] in
      let complete = await endOfTurn.isComplete(turn: window)
      guard !Task.isCancelled else { return }
      self?.send(.endOfTurn(id: id, complete: complete))
    }
  }

  private func submit(_ text: String, interrupted: Bool) {
    let heard = interrupted ? heardOnInterruption : nil
    heardOnInterruption = nil
    reply = ReplyQueue(chunker: options.sentences, accepting: true)
    onTurn?(VoiceTurn(text: text, heardOfInterruptedReply: heard))
  }

  // MARK: The avatar's reply

  /// The avatar's message has grown to `markdown`, whole when `final`. Ignored unless the person's
  /// turn went out and the reply was not cancelled since.
  public func replyUpdated(_ markdown: String, final: Bool) {
    guard reply.accepting else { return }
    let spoken = SpeechText.speakable(markdown, streaming: !final)
    reply.sentences += reply.chunker.update(spoken, final: final)
    reply.complete = reply.complete || final
    pump()
  }

  /// Starts speaking when sentences wait and nothing speaks; ends the reply when it is all said.
  private func pump() {
    guard speaking == nil else { return }
    guard !reply.sentences.isEmpty else {
      if reply.accepting, reply.complete { finishReply() }
      return
    }
    let epoch = speechEpoch
    speaking = Task { [weak self] in
      await self?.speakQueue()
      guard let self, self.speechEpoch == epoch else { return }
      self.speaking = nil
      self.pump()
    }
  }

  /// Speaks the queue in order, the voice already working on the next sentence while one plays.
  private func speakQueue() async {
    var upcoming: (sentence: String, speech: Task<SynthesizedSpeech, any Error>)?
    while !Task.isCancelled, let (sentence, speech) = take(upcoming) {
      upcoming = reply.sentences.first.map { ($0, synthesis(of: $0)) }
      do {
        let heard = try await speech.value
        try Task.checkCancellation()
        try await play(heard, saying: sentence)
        reply.played.append(sentence)
      } catch is CancellationError {
        upcoming?.speech.cancel()
        return
      } catch {
        failure = error.localizedDescription
      }
    }
    upcoming?.speech.cancel()
  }

  /// The next sentence to speak with its audio on the way: the one already started, or the first
  /// that waits.
  private func take(
    _ started: (sentence: String, speech: Task<SynthesizedSpeech, any Error>)?
  ) -> (String, Task<SynthesizedSpeech, any Error>)? {
    if let started, !reply.sentences.isEmpty {
      reply.sentences.removeFirst()
      return started
    }
    guard let first = reply.sentences.first else { return nil }
    reply.sentences.removeFirst()
    return (first, synthesis(of: first))
  }

  private func synthesis(of sentence: String) -> Task<SynthesizedSpeech, any Error> {
    Task { [synthesizer, voice = options.voice] in
      try await synthesizer.synthesize(sentence, options: voice)
    }
  }

  private func play(_ speech: SynthesizedSpeech, saying sentence: String) async throws {
    currentSpeech = speech
    currentSentence = sentence
    send(.sentenceStarted(sentence))
    ticker = Task { [weak self] in
      while !Task.isCancelled {
        self?.updateOutputLevel()
        do { try await Task.sleep(for: .milliseconds(16)) } catch { return }
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

  private func finishReply() {
    reply.accepting = false
    send(.replyFinished)
  }

  private func stopPlayback() {
    heardOnInterruption = heardNow()
    cutSpeaking()
    audio.stopPlayback()
  }

  private func cancelReply() {
    cutSpeaking()
    reply = ReplyQueue(chunker: options.sentences)
    onCancelReply?()
  }

  private func cutSpeaking() {
    speechEpoch += 1
    speaking?.cancel()
    speaking = nil
  }

  /// What has been heard of the reply: the sentences played, and the whole words of the one playing
  /// that its position in the audio covers.
  private func heardNow() -> String {
    let seconds =
      currentSpeech.map { Double($0.samples.count) / Double(max($0.sampleRate, 1)) } ?? 0
    let fraction = seconds > 0 ? audio.playbackPosition / seconds : 0
    return HeardSpeech.text(played: reply.played, current: currentSentence, fraction: fraction)
  }

  /// What the reply being spoken has left to say, and what it has said.
  private struct ReplyQueue {
    var chunker: SentenceChunker
    var accepting: Bool
    var sentences: [String] = []
    var played: [String] = []
    var complete = false

    init(chunker: SentenceChunker, accepting: Bool = false) {
      self.chunker = chunker
      self.accepting = accepting
    }
  }
}
