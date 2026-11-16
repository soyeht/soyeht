import Foundation
import SoyehtProtocol

/// The numbers the turn machine runs on, in whole milliseconds. The defaults are `x-clientPolicy.voice`
/// of `contracts/protocol.json`, the same numbers the desktop client reads; a case of
/// `contracts/vectors/voice-turns.json` may override any of them.
public struct VoiceTurnConfig: Equatable, Sendable {
  /// The length of one voice-activity frame.
  public var frameMs = ClientPolicy.voiceFrameMs
  /// A frame is voiced from this speech probability up, or from `endThreshold` up while the frame
  /// before it was voiced.
  public var startThreshold = ClientPolicy.voiceStartThreshold
  public var endThreshold = ClientPolicy.voiceEndThreshold
  /// How long voiced frames must run, without a break, to count as speech. A shorter run is a blip:
  /// it neither starts a turn nor breaks a silence.
  public var speechStartMs = ClientPolicy.voiceSpeechStartMs
  /// Audio kept from before the start was noticed, so the first word is not clipped.
  public var preRollMs = ClientPolicy.voicePreRollMs
  /// How far back the microphone's audio is still kept; a recognizer never starts from before it.
  public var historyMs = ClientPolicy.voiceHistoryMs
  /// Silence after speech before the end-of-turn model is asked.
  public var silenceMs = ClientPolicy.voiceSilenceMs
  /// The turn ends after this much silence, whatever the model says or whether it answered.
  public var incompleteSilenceMs = ClientPolicy.voiceIncompleteSilenceMs
  /// The recognizer has this long, after the end of a turn, to give its final text; then the last
  /// partial one is used.
  public var finalizeTimeoutMs = ClientPolicy.voiceFinalizeTimeoutMs
  /// How long the person must talk over the avatar, or over the wait for its reply, before it is
  /// worth hearing what they say.
  public var bargeInMs = ClientPolicy.voiceBargeInMs
  /// A listen for an interruption that decided nothing in this time is dropped.
  public var probeTimeoutMs = ClientPolicy.voiceProbeTimeoutMs
  /// The microphone is not heard while the avatar speaks; only an explicit interrupt stops it.
  public var halfDuplex = false
  /// In half duplex, the microphone stays deaf this long after the avatar's voice ends, for the
  /// tail of the speaker.
  public var halfDuplexTailMs = ClientPolicy.voiceHalfDuplexTailMs
  /// Short replies of a listener, which do not take the turn from the avatar, separated by `|`.
  public var continuers = ClientPolicy.voiceContinuers.split(separator: "|").map(String.init)

  public init() {}
}

public enum VoiceTurnState: String, Equatable, Sendable {
  case idle, listening, thinking, speaking
}

/// Where the machine is inside a state: what the avatar's face and the controller act on.
public enum VoiceTurnPhase: String, Equatable, Sendable, CaseIterable {
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
  /// Thinking, someone talks and the machine listens to what they say.
  case probingWait
  /// Speaking, the reply is audible.
  case playing
  /// Speaking, someone has talked over the reply and the machine listens to what they say.
  case probing

  public var state: VoiceTurnState {
    switch self {
    case .off: .idle
    case .waiting, .hearing, .deciding: .listening
    case .finalizing, .waitingReply, .probingWait: .thinking
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
  /// A sentence of a reply began to play.
  case sentenceStarted(String)
  /// The replies were played to their end, or ended with nothing to say.
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
  /// Drop the recognizer and what it heard.
  case discardTurn
  /// Send the person's words to the agent. `interrupted` is set when they talked over a reply,
  /// so the controller can say which part of it they heard.
  case submit(text: String, interrupted: Bool)
  case cancelReply
  case stopPlayback
  /// Start a recognizer on what is being said over the reply, or over the wait for it.
  case probeInterruption(fromMs: Int)
  /// The person stopped talking: stop feeding the probe's recognizer and wait for its final text.
  case finishProbe
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
  /// The time of the last frame of speech (a voiced run of `speechStartMs` or more).
  private var speechEnd = 0
  private var ignoreUntil = 0
  /// The ids of the end-of-turn checks only grow: an answer for an older check is never taken.
  private var checkID = 0
  /// The recognizer's text for the turn so far, and its final text when it ended before the turn did.
  private var partial = ""
  private var finalText: String?
  private var finalizeStart = 0
  private var interrupted = false
  private var needsQuiet = false
  private var probeStart = 0
  private var probeFinishing = false
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
    case .endOfTurn(let id, let complete): return endOfTurn(id: id, complete: complete, at: at)
    case .transcript(let text, let final): return transcript(text, final: final)
    case .sentenceStarted(let text): return sentenceStarted(text)
    case .replyFinished: return replyFinished(at: at)
    case .interrupt: return interrupt()
    }
  }

  // MARK: Frames

  private mutating func frame(_ probability: Double, at: Int) -> [VoiceTurnAction] {
    guard at >= ignoreUntil else {
      voiced = false
      return []
    }
    let was = voiced
    voiced =
      probability >= config.startThreshold || (was && probability >= config.endThreshold)
    if voiced, !was { runStart = at - config.frameMs }
    let run = voiced ? at - runStart : 0
    let inSpeech = voiced && run >= config.speechStartMs
    if inSpeech { speechEnd = at }
    let silence = inSpeech ? 0 : at - speechEnd
    switch phase {
    case .off: return []
    case .waiting:
      guard inSpeech else { return [] }
      phase = .hearing
      beginTurn()
      return [.startTurn(fromMs: restart(at))]
    case .hearing:
      guard !inSpeech, silence >= config.silenceMs else { return [] }
      checkID += 1
      phase = .deciding
      return [.checkEndOfTurn(id: checkID)]
    case .deciding:
      if inSpeech {
        phase = .hearing
        return []
      }
      return silence >= config.incompleteSilenceMs ? endTurn(at: at) : []
    case .finalizing:
      guard at - finalizeStart >= config.finalizeTimeoutMs else { return [] }
      return [.discardTurn] + complete(partial).filter { $0 != .discardTurn }
    case .waitingReply:
      guard voiced, run >= config.bargeInMs else { return [] }
      return startProbe(.probingWait, at: at)
    case .playing:
      guard !config.halfDuplex else { return [] }
      if !voiced { needsQuiet = false }
      guard voiced, !needsQuiet, run >= config.bargeInMs else { return [] }
      return startProbe(.probing, at: at)
    case .probing, .probingWait:
      if at - probeStart >= config.probeTimeoutMs { return dropProbe(stillVoiced: voiced) }
      guard !inSpeech, silence >= config.silenceMs, !probeFinishing else { return [] }
      probeFinishing = true
      return [.finishProbe]
    }
  }

  /// Where a recognizer started now begins in the audio: before the voice began, by the pre-roll,
  /// and never before what is still kept.
  private func restart(_ at: Int) -> Int {
    max(0, runStart - config.preRollMs, at - config.historyMs)
  }

  private mutating func beginTurn() {
    partial = ""
    finalText = nil
  }

  private mutating func startProbe(_ to: VoiceTurnPhase, at: Int) -> [VoiceTurnAction] {
    phase = to
    probeStart = at
    probeFinishing = false
    beginTurn()
    return [.probeInterruption(fromMs: restart(at))]
  }

  private mutating func dropProbe(stillVoiced: Bool) -> [VoiceTurnAction] {
    phase = phase == .probing ? .playing : .waitingReply
    needsQuiet = stillVoiced
    return [.dropProbe]
  }

  /// The turn is over: with the recognizer's text if it already ended, else by asking for it.
  private mutating func endTurn(at: Int) -> [VoiceTurnAction] {
    if let finalText { return complete(finalText) }
    phase = .finalizing
    finalizeStart = at
    return [.finalizeTurn]
  }

  /// The words of the turn are known: they go to the agent, or the turn was empty.
  private mutating func complete(_ text: String) -> [VoiceTurnAction] {
    let words = text.trimmingCharacters(in: .whitespacesAndNewlines)
    let wasInterrupted = interrupted
    interrupted = false
    finalText = nil
    guard !words.isEmpty else {
      phase = .waiting
      return [.discardTurn]
    }
    phase = .waitingReply
    return [.submit(text: words, interrupted: wasInterrupted)]
  }

  // MARK: Other inputs

  private mutating func endOfTurn(id: Int, complete: Bool, at: Int) -> [VoiceTurnAction] {
    guard phase == .deciding, id == checkID, complete else { return [] }
    return endTurn(at: at)
  }

  private mutating func transcript(_ text: String, final: Bool) -> [VoiceTurnAction] {
    switch phase {
    case .hearing, .deciding:
      partial = text
      if final { finalText = text }
      return []
    case .finalizing:
      partial = text
      return final ? complete(text) : []
    case .probing, .probingWait:
      let tokens = VoiceText.tokens(text)
      guard !tokens.isEmpty, !isContinuer(tokens), !isEcho(tokens) else {
        return final ? dropProbe(stillVoiced: voiced) : []
      }
      let playing = phase == .probing
      phase = .hearing
      interrupted = playing
      partial = text
      finalText = final ? text : nil
      return playing ? [.stopPlayback, .cancelReply] : [.cancelReply]
    default: return []
    }
  }

  private mutating func sentenceStarted(_ text: String) -> [VoiceTurnAction] {
    switch phase {
    case .waiting, .waitingReply:
      phase = .playing
      spoken = [VoiceText.tokens(text)]
      needsQuiet = false
      if config.halfDuplex { voiced = false }
    case .playing, .probing:
      spoken = Array((spoken + [VoiceText.tokens(text)]).suffix(2))
    default: break
    }
    return []
  }

  private mutating func replyFinished(at: Int) -> [VoiceTurnAction] {
    let actions: [VoiceTurnAction]
    switch phase {
    case .waitingReply: actions = []
    case .playing, .probing:
      actions = phase == .probing ? [.dropProbe] : []
      if config.halfDuplex {
        voiced = false
        ignoreUntil = at + config.halfDuplexTailMs
      }
    case .probingWait: actions = [.dropProbe]
    default: return []
    }
    phase = .waiting
    return actions
  }

  private mutating func interrupt() -> [VoiceTurnAction] {
    guard phase != .off, phase != .waiting else { return [] }
    let actions = cancelling()
    phase = .waiting
    interrupted = false
    voiced = false
    return actions
  }

  private mutating func stop() -> [VoiceTurnAction] {
    let actions = cancelling()
    reset()
    phase = .off
    return actions
  }

  /// What drops everything that is under way in the phase the machine is in.
  private func cancelling() -> [VoiceTurnAction] {
    switch phase {
    case .off, .waiting: []
    case .hearing, .deciding, .finalizing: [.discardTurn]
    case .waitingReply: [.cancelReply]
    case .probingWait: [.dropProbe, .cancelReply]
    case .playing: [.stopPlayback, .cancelReply]
    case .probing: [.dropProbe, .stopPlayback, .cancelReply]
    }
  }

  private mutating func reset() {
    voiced = false
    runStart = 0
    speechEnd = 0
    ignoreUntil = 0
    partial = ""
    finalText = nil
    interrupted = false
    needsQuiet = false
    probeFinishing = false
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
      } else {
        return continuers.contains { $0.starts(with: rest) }
      }
    }
    return true
  }

  /// The text repeats words of the last sentences played: the speaker heard by the microphone. One
  /// word is never an echo, so that "stop" always works; the last word may be unfinished, since the
  /// recognizer is still hearing it.
  private func isEcho(_ tokens: [String]) -> Bool {
    let heard = spoken.flatMap { $0 }
    guard tokens.count >= 2, heard.count >= tokens.count else { return false }
    return (0...(heard.count - tokens.count)).contains { start in
      tokens.indices.allSatisfy { offset in
        offset == tokens.count - 1
          ? heard[start + offset].hasPrefix(tokens[offset])
          : heard[start + offset] == tokens[offset]
      }
    }
  }
}

/// How the turn machine compares words: Unicode NFC, lowercase, a word being a run of letters,
/// marks and digits, and anything else a break. Each character of a script that writes no blanks
/// (Han, kana, Thai, Lao, Khmer, Myanmar) is a word of its own.
enum VoiceText {
  static func tokens(_ text: String) -> [String] {
    var words: [String] = []
    var current = ""
    for scalar in text.precomposedStringWithCanonicalMapping.lowercased().unicodeScalars {
      if isBlankless(scalar) {
        if !current.isEmpty { words.append(current) }
        current = ""
        words.append(String(scalar))
      } else if CharacterSet.alphanumerics.contains(scalar) {
        current.unicodeScalars.append(scalar)
      } else {
        if !current.isEmpty { words.append(current) }
        current = ""
      }
    }
    if !current.isEmpty { words.append(current) }
    return words
  }

  /// A script whose words are not set apart by blanks.
  static func isBlankless(_ scalar: Unicode.Scalar) -> Bool {
    switch scalar.value {
    case 0x1000...0x109F, 0x1780...0x17FF, 0x0E00...0x0EFF, 0x3040...0x30FF, 0x3400...0x4DBF,
      0x4E00...0x9FFF, 0xF900...0xFAFF, 0x20000...0x2A6DF:
      true
    default: false
    }
  }
}
