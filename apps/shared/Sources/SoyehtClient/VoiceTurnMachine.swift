import Foundation

/// The numbers the turn machine runs on, in whole milliseconds. The defaults are the ones in
/// `defaults` of `contracts/vectors/voice-turns.json`; a test proves they are the same.
public struct VoiceTurnConfig: Equatable, Sendable {
  /// The length of one voice-activity frame: 512 samples at 16 kHz.
  public var frameMs = 32
  /// A frame is voiced from this speech probability up, or from `endThreshold` up while the frame
  /// before it was voiced.
  public var startThreshold = 0.5
  public var endThreshold = 0.35
  /// How long the voiced frames must run, without a break, before the person has started to talk.
  public var speechStartMs = 96
  /// Audio kept from before the start was noticed, so the first word is not clipped.
  public var preRollMs = 320
  /// Silence after speech before the end-of-turn model is asked.
  public var silenceMs = 350
  /// When the model says the person is not done, the turn still ends after this much silence.
  public var incompleteSilenceMs = 2_000
  /// How long the person must talk over the avatar before it is worth hearing what they say.
  public var bargeInMs = 350
  /// A listen for an interruption that decided nothing in this time is dropped.
  public var probeTimeoutMs = 1_500
  /// The microphone is not heard while the avatar speaks; only an explicit interrupt stops it.
  public var halfDuplex = false
  /// Short replies of a listener ("uh-huh", "ok"), which do not take the turn from the avatar.
  public var continuers = [
    "uh huh", "mhm", "mm hmm", "mmm", "hmm", "yeah", "yes", "ok", "okay", "right", "sure",
    "got it", "i see", "aham", "uhum", "sim", "certo", "entendi", "tá", "é isso",
  ]

  public init() {}
}

public enum VoiceTurnState: String, Equatable, Sendable {
  case idle, listening, thinking, speaking
}

/// Where the machine is inside a state: what the avatar's face and the controller act on.
public enum VoiceTurnPhase: String, Equatable, Sendable {
  case off
  /// Listening, nobody is talking.
  case waiting
  /// Listening, the person is talking or paused for less than `silenceMs`.
  case hearing
  /// Listening, the pause is long enough that the end-of-turn model was asked.
  case deciding
  /// Thinking, the turn ended and the recognizer is producing its final text.
  case finalizing
  /// Thinking, the person's words went to the agent and no reply is audible yet.
  case waitingReply
  /// Speaking, the reply is audible.
  case playing
  /// Speaking, someone has talked over the reply and the machine listens to what they say.
  case probing

  public var state: VoiceTurnState {
    switch self {
    case .off: .idle
    case .waiting, .hearing, .deciding: .listening
    case .finalizing, .waitingReply: .thinking
    case .playing, .probing: .speaking
    }
  }
}

/// What reaches the machine. Time is not an input of its own: it advances with the frames, which
/// arrive every `frameMs` while the microphone is on.
public enum VoiceTurnInput: Equatable, Sendable {
  case start
  case stop
  /// One voice-activity frame, stamped with the time its audio ended.
  case frame(probability: Double)
  /// The end-of-turn model's answer to the check `id`.
  case endOfTurn(id: Int, complete: Bool)
  case transcript(String, final: Bool)
  /// A sentence of the reply began to play.
  case sentenceStarted(String)
  /// The reply was played to its end, or ended with nothing to say.
  case replyFinished
  /// The person asked to stop, with a key or a click.
  case interrupt
}

/// What the machine asks of the controller. Times are milliseconds on the microphone's clock.
public enum VoiceTurnAction: Equatable, Sendable {
  /// Start a recognizer with the audio from `fromMs` on.
  case startTurn(fromMs: Int)
  case checkEndOfTurn(id: Int)
  /// Stop feeding the recognizer and wait for its final text.
  case finalizeTurn
  case discardTurn
  /// Send the person's words to the agent. `interrupted` is set when they talked over a reply,
  /// so the controller can say which part of it they heard.
  case submit(text: String, interrupted: Bool)
  case cancelReply
  case stopPlayback
  /// Start a recognizer on what is being said over the reply.
  case probeInterruption(fromMs: Int)
  case dropProbe
}

/// The turn-taking rules of the voice loop (ADR 0166 section 5): when the person starts and ends a
/// turn, when the agent's words go out, and when the person talking over the avatar takes the turn.
/// It is a pure state machine: no clock, no audio, no IO. The Windows and Linux app implements the
/// same rules and passes the same cases (`contracts/vectors/voice-turns.json`).
public struct VoiceTurnMachine: Sendable {
  public private(set) var phase: VoiceTurnPhase = .off
  public var state: VoiceTurnState { phase.state }

  private let config: VoiceTurnConfig
  private let continuers: [[String]]
  private var voiced = false
  private var runStart = 0
  private var lastVoicedEnd = 0
  private var lastFrameAt = 0
  private var checkID = 0
  private var incomplete = false
  private var interrupted = false
  private var needsQuiet = false
  private var probeStart = 0
  /// The tokens of the last two sentences played: what an echo of the speaker would repeat.
  private var spoken: [[String]] = []

  public init(_ config: VoiceTurnConfig = VoiceTurnConfig()) {
    self.config = config
    continuers = config.continuers.map(VoiceText.tokens).filter { !$0.isEmpty }
  }

  public mutating func handle(_ input: VoiceTurnInput, at: Int) -> [VoiceTurnAction] {
    switch input {
    case .start:
      guard phase == .off else { return [] }
      reset()
      phase = .waiting
      return []
    case .stop: return stop()
    case .frame(let probability): return frame(probability, at: at)
    case .endOfTurn(let id, let complete): return endOfTurn(id: id, complete: complete)
    case .transcript(let text, let final): return transcript(text, final: final)
    case .sentenceStarted(let text): return sentenceStarted(text)
    case .replyFinished: return replyFinished()
    case .interrupt: return interrupt()
    }
  }

  // MARK: Frames

  private mutating func frame(_ probability: Double, at: Int) -> [VoiceTurnAction] {
    lastFrameAt = at
    let was = voiced
    voiced =
      probability >= config.startThreshold || (was && probability >= config.endThreshold)
    if voiced {
      if !was { runStart = at - config.frameMs }
      lastVoicedEnd = at
    }
    let run = voiced ? at - runStart : 0
    let silence = voiced ? 0 : at - lastVoicedEnd
    switch phase {
    case .off, .finalizing: return []
    case .waiting:
      guard voiced, run >= config.speechStartMs else { return [] }
      phase = .hearing
      return [.startTurn(fromMs: preRolled())]
    case .hearing:
      guard !voiced, silence >= config.silenceMs else { return [] }
      checkID += 1
      incomplete = false
      phase = .deciding
      return [.checkEndOfTurn(id: checkID)]
    case .deciding:
      if voiced, run >= config.speechStartMs {
        phase = .hearing
      } else if incomplete, !voiced, silence >= config.incompleteSilenceMs {
        return finalize()
      }
      return []
    case .waitingReply:
      guard voiced, run >= config.speechStartMs else { return [] }
      phase = .hearing
      interrupted = false
      return [.cancelReply, .startTurn(fromMs: preRolled())]
    case .playing:
      guard !config.halfDuplex else { return [] }
      if !voiced { needsQuiet = false }
      guard voiced, !needsQuiet, run >= config.bargeInMs else { return [] }
      phase = .probing
      probeStart = at
      return [.probeInterruption(fromMs: preRolled())]
    case .probing:
      if !voiced, silence >= config.silenceMs { return dropProbe(stillVoiced: false) }
      if at - probeStart >= config.probeTimeoutMs { return dropProbe(stillVoiced: voiced) }
      return []
    }
  }

  private func preRolled() -> Int { max(0, runStart - config.preRollMs) }

  private mutating func finalize() -> [VoiceTurnAction] {
    phase = .finalizing
    return [.finalizeTurn]
  }

  private mutating func dropProbe(stillVoiced: Bool) -> [VoiceTurnAction] {
    phase = .playing
    needsQuiet = stillVoiced
    return [.dropProbe]
  }

  // MARK: Other inputs

  private mutating func endOfTurn(id: Int, complete: Bool) -> [VoiceTurnAction] {
    guard phase == .deciding, id == checkID else { return [] }
    if complete { return finalize() }
    incomplete = true
    if !voiced, lastFrameAt - lastVoicedEnd >= config.incompleteSilenceMs { return finalize() }
    return []
  }

  private mutating func transcript(_ text: String, final: Bool) -> [VoiceTurnAction] {
    switch phase {
    case .finalizing:
      guard final else { return [] }
      let words = text.trimmingCharacters(in: .whitespacesAndNewlines)
      let wasInterrupted = interrupted
      interrupted = false
      guard !words.isEmpty else {
        phase = .waiting
        return [.discardTurn]
      }
      phase = .waitingReply
      return [.submit(text: words, interrupted: wasInterrupted)]
    case .probing:
      let tokens = VoiceText.tokens(text)
      guard !tokens.isEmpty, !isContinuer(tokens), !isEcho(tokens) else { return [] }
      phase = .hearing
      interrupted = true
      return [.stopPlayback, .cancelReply]
    default: return []
    }
  }

  private mutating func sentenceStarted(_ text: String) -> [VoiceTurnAction] {
    switch phase {
    case .waitingReply:
      phase = .playing
      spoken = [VoiceText.tokens(text)]
    case .playing, .probing:
      spoken = Array((spoken + [VoiceText.tokens(text)]).suffix(2))
    default: break
    }
    return []
  }

  private mutating func replyFinished() -> [VoiceTurnAction] {
    let actions: [VoiceTurnAction]
    switch phase {
    case .waitingReply, .playing: actions = []
    case .probing: actions = [.dropProbe]
    default: return []
    }
    phase = .waiting
    if config.halfDuplex { voiced = false }
    return actions
  }

  private mutating func interrupt() -> [VoiceTurnAction] {
    let actions: [VoiceTurnAction]
    switch phase {
    case .hearing, .deciding, .finalizing: actions = [.discardTurn]
    case .waitingReply: actions = [.cancelReply]
    case .playing: actions = [.stopPlayback, .cancelReply]
    case .probing: actions = [.dropProbe, .stopPlayback, .cancelReply]
    case .off, .waiting: return []
    }
    phase = .waiting
    interrupted = false
    voiced = false
    return actions
  }

  private mutating func stop() -> [VoiceTurnAction] {
    let actions: [VoiceTurnAction]
    switch phase {
    case .off, .waiting: actions = []
    case .hearing, .deciding, .finalizing: actions = [.discardTurn]
    case .waitingReply: actions = [.cancelReply]
    case .playing: actions = [.stopPlayback, .cancelReply]
    case .probing: actions = [.dropProbe, .stopPlayback, .cancelReply]
    }
    reset()
    phase = .off
    return actions
  }

  private mutating func reset() {
    voiced = false
    runStart = 0
    lastVoicedEnd = 0
    lastFrameAt = 0
    checkID = 0
    incomplete = false
    interrupted = false
    needsQuiet = false
    spoken = []
  }

  // MARK: Interruption filter

  /// The whole text is made of listener noises, or the start of one.
  private func isContinuer(_ tokens: [String]) -> Bool {
    var index = 0
    while index < tokens.count {
      let rest = tokens[index...]
      if let match = continuers.first(where: { rest.starts(with: $0) }) {
        index += match.count
      } else if continuers.contains(where: { $0.starts(with: rest) }) {
        return true
      } else {
        return false
      }
    }
    return true
  }

  /// The text repeats words of the last sentences played: the speaker heard by the microphone.
  /// The last word may be unfinished, since the recognizer is still hearing it.
  private func isEcho(_ tokens: [String]) -> Bool {
    spoken.contains { sentence in
      guard sentence.count >= tokens.count else { return false }
      return (0...(sentence.count - tokens.count)).contains { start in
        tokens.indices.allSatisfy { offset in
          let heard = sentence[start + offset]
          return offset == tokens.count - 1
            ? heard.hasPrefix(tokens[offset]) : heard == tokens[offset]
        }
      }
    }
  }
}

/// How the turn machine compares words: lowercase letters and digits, everything else a break.
enum VoiceText {
  static func tokens(_ text: String) -> [String] {
    text.lowercased().unicodeScalars
      .map { CharacterSet.alphanumerics.contains($0) ? Character($0) : " " }
      .split(separator: " ")
      .map { String($0) }
  }
}
