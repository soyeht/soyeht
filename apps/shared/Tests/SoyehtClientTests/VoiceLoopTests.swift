import Foundation
import Testing

@testable import SoyehtClient

// MARK: - Fakes

@MainActor private final class FakeAudio: VoiceAudio {
  var onFailure: (@MainActor (String) -> Void)?
  var startError: (any Error)?
  var startCount = 0
  var started = false
  var replays: [Int] = []
  var played: [SynthesizedSpeech] = []
  var stoppedPlayback = 0
  var playbackPosition = 0.0
  private var waiting: CheckedContinuation<Void, Never>?

  func start(onSamples: @escaping @Sendable ([Float]) -> Void) throws {
    if let startError { throw startError }
    startCount += 1
    started = true
  }

  func stop() { started = false }

  func listening(from milliseconds: Int, _ start: () throws -> Void) rethrows {
    replays.append(milliseconds)
    try start()
  }

  func play(_ speech: SynthesizedSpeech) async throws {
    played.append(speech)
    await withCheckedContinuation { waiting = $0 }
  }

  func stopPlayback() {
    stoppedPlayback += 1
    finishPlayback()
  }

  func finishPlayback() {
    waiting?.resume()
    waiting = nil
  }
}

@MainActor private final class FakeRecognizer: SpeechRecognizing {
  var authorization = DictationAuthorization.authorized
  var gates: [CheckedContinuation<Void, Never>] = []
  var held = false
  var onEvent: (@MainActor (SpeechEvent) -> Void)?
  var finished = false
  var cancelled = false

  func authorize() async -> DictationAuthorization {
    if held { await withCheckedContinuation { gates.append($0) } }
    return authorization

  }

  /// Answers every permission prompt that waits.
  func release() {
    held = false
    for gate in gates { gate.resume() }
    gates = []
  }

  func start(
    _ options: SpeechSessionOptions, onEvent: @escaping @MainActor (SpeechEvent) -> Void
  ) throws {
    self.onEvent = onEvent
  }

  func finish() { finished = true }
  func cancel() { cancelled = true }
}

private actor FakeVoice: SpeechSynthesizing {
  private(set) var spoken: [String] = []

  func synthesize(_ text: String, options: SpeechSynthesisOptions) async throws
    -> SynthesizedSpeech
  {
    spoken.append(text)
    return SynthesizedSpeech(
      samples: [Float](repeating: 0.2, count: text.count * 160), sampleRate: 16_000)
  }
}

private final class ScriptedActivity: VoiceActivityDetecting, @unchecked Sendable {
  nonisolated(unsafe) var level = 0.0
  nonisolated(unsafe) var fails = false

  func probability(of frame: [Float]) throws -> Double {
    if fails { throw VoiceModelError.prediction("scripted") }
    return level
  }

  func reset() {}
}

private struct ScriptedEndOfTurn: EndOfTurnDetecting {
  var complete = true
  var fails = false

  func isComplete(turn audio: [Float]) async throws -> Bool {
    if fails { throw VoiceModelError.prediction("scripted") }
    return complete
  }
}

@MainActor private final class RecognizerBox {
  var all: [FakeRecognizer] = []
}

/// Everything a test of the loop needs, wired.
@MainActor private final class Rig {
  let audio = FakeAudio()
  let voice = FakeVoice()
  let activity = ScriptedActivity()
  var turns: [VoiceTurn] = []
  var cancelled = 0
  let controller: VoiceLoopController
  private let box = RecognizerBox()

  init(
    endOfTurn: any EndOfTurnDetecting = SilenceEndOfTurn(),
    options: VoiceLoopOptions = VoiceLoopOptions(), recognizer: FakeRecognizer? = nil
  ) {
    let made = box
    controller = VoiceLoopController(
      audio: audio,
      recognizer: {
        let next = recognizer ?? FakeRecognizer()
        made.all.append(next)
        return next
      }, synthesizer: voice, activity: activity, endOfTurn: endOfTurn, options: options)
    controller.onTurn = { [weak self] in self?.turns.append($0) }
    controller.onCancelReply = { [weak self] in self?.cancelled += 1 }
  }

  var lastRecognizer: FakeRecognizer? { box.all.last }

  /// `frames` frames of audio the voice-activity model reads as `level`.
  func hear(_ frames: Int, level: Double) async {
    activity.level = level
    await controller.feed([Float](repeating: 0, count: frames * VoiceFrames.size))
  }

  func eventually(_ condition: @MainActor () -> Bool) async throws {
    for _ in 0..<400 where !condition() { try await Task.sleep(for: .milliseconds(5)) }
    #expect(condition())
  }

  /// A turn answered, the first sentence of the reply playing.
  func startSpeaking() async throws {
    try await controller.start()
    try await say("hello")
    controller.replyUpdated("Hi there, friend. How", final: false, id: "m1")
    try await eventually { self.audio.played.count == 1 }
  }

  /// A whole turn up to the person's words being sent: speech, a pause, the recognizer's final.
  func say(_ words: String) async throws {
    await hear(3, level: 0.9)
    await hear(8, level: 0.9)
    lastRecognizer?.onEvent?(.partial(words))
    await hear(11, level: 0.1)
    try await eventually {
      self.controller.phase == .finalizing || self.controller.phase == .waitingReply
    }
    if controller.phase == .finalizing { lastRecognizer?.onEvent?(.final(words)) }
  }
}

private let reply = "Hi there, friend. How can I help you today?"

// MARK: - A turn and its reply

@MainActor @Test func aSpokenTurnReachesTheAgentAndItsReplyIsSpokenInOrder() async throws {
  let rig = Rig()
  try await rig.controller.start()
  #expect(rig.audio.started)
  #expect(rig.controller.phase == .waiting)

  await rig.hear(3, level: 0.9)
  #expect(rig.controller.phase == .hearing)
  #expect(rig.audio.replays == [0])
  await rig.hear(8, level: 0.9)
  rig.lastRecognizer?.onEvent?(.partial("what time"))
  #expect(rig.controller.partialTranscript == "what time")
  await rig.hear(11, level: 0.1)
  try await rig.eventually { rig.controller.phase == .finalizing }
  #expect(rig.lastRecognizer?.finished == true)
  rig.lastRecognizer?.onEvent?(.final("what time is it"))
  #expect(rig.turns == [VoiceTurn(text: "what time is it")])
  #expect(rig.controller.state == .thinking)

  rig.controller.replyUpdated("Hi there, friend. How can", final: false, id: "m1")
  try await rig.eventually { rig.audio.played.count == 1 }
  #expect(rig.controller.state == .speaking)
  #expect(rig.controller.currentSentence == "Hi there, friend.")
  rig.audio.finishPlayback()
  rig.controller.replyUpdated(reply, final: true, id: "m1")
  try await rig.eventually { rig.audio.played.count == 2 }
  rig.audio.finishPlayback()
  try await rig.eventually { rig.controller.phase == .waiting }
  #expect(await rig.voice.spoken == ["Hi there, friend.", "How can I help you today?"])
  #expect(rig.controller.currentSpeech == nil)
}

@MainActor @Test func theLastPartialTextIsUsedWhenTheRecognizerNeverFinishes() async throws {
  var options = VoiceLoopOptions()
  options.turn.finalizeTimeoutMs = 300
  let rig = Rig(options: options)
  try await rig.controller.start()
  await rig.hear(3, level: 0.9)
  rig.lastRecognizer?.onEvent?(.partial("open the file"))
  await rig.hear(11, level: 0.1)
  try await rig.eventually { rig.controller.phase == .finalizing }
  await rig.hear(12, level: 0.1)
  #expect(rig.turns == [VoiceTurn(text: "open the file")])
  #expect(rig.lastRecognizer?.cancelled == true)
}

@MainActor @Test func aRecognizerThatFailsMidTurnStillGivesTheTurnItsWords() async throws {
  let rig = Rig()
  try await rig.controller.start()
  await rig.hear(3, level: 0.9)
  rig.lastRecognizer?.onEvent?(.partial("open the file"))
  rig.lastRecognizer?.onEvent?(.failed("the recognizer stopped"))
  await rig.hear(11, level: 0.1)
  try await rig.eventually { rig.turns.count == 1 }
  #expect(rig.turns == [VoiceTurn(text: "open the file")])
  #expect(rig.controller.failure == "the recognizer stopped")
}

@MainActor @Test func aTurnTheModelCallsUnfinishedEndsAtTheLongSilence() async throws {
  let rig = Rig(endOfTurn: ScriptedEndOfTurn(complete: false))
  try await rig.controller.start()
  await rig.hear(3, level: 0.9)
  await rig.hear(11, level: 0.1)
  try await rig.eventually { rig.controller.phase == .deciding }
  try await Task.sleep(for: .milliseconds(50))
  #expect(rig.controller.phase == .deciding)
  await rig.hear(60, level: 0.1)
  #expect(rig.controller.phase == .finalizing)
}

@MainActor @Test func aModelThatFailsToAnswerEndsTheTurnAtThePauseAndSaysSo() async throws {
  let rig = Rig(endOfTurn: ScriptedEndOfTurn(fails: true))
  try await rig.controller.start()
  await rig.hear(3, level: 0.9)
  await rig.hear(11, level: 0.1)
  try await rig.eventually { rig.controller.phase == .finalizing }
  #expect(rig.controller.failure?.contains("voice model") == true)
}

@MainActor @Test func aVoiceModelThatFailsIsReportedAndTheLoudnessDetectorTakesOver() async throws {
  let rig = Rig()
  try await rig.controller.start()
  rig.activity.fails = true
  await rig.hear(2, level: 0.9)
  #expect(rig.controller.failure?.contains("voice model") == true)
  #expect(rig.controller.phase == .waiting)
  // The fallback reads silence as silence: the loop goes on.
  await rig.hear(5, level: 0.9)
  #expect(rig.controller.phase == .waiting)
}

@MainActor @Test func nothingIsSentWhenTheRecognizerHeardNothing() async throws {
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("   ")
  #expect(rig.turns.isEmpty)
  #expect(rig.controller.phase == .waiting)
}

// MARK: - Replies

@MainActor @Test func aReplyNoVoiceTurnAskedForIsSpoken() async throws {
  let rig = Rig()
  try await rig.controller.start()
  rig.controller.replyUpdated("Good morning, how are you?", final: true, id: "x")
  try await rig.eventually { rig.audio.played.count == 1 }
  #expect(rig.controller.state == .speaking)
  rig.audio.finishPlayback()
  try await rig.eventually { rig.controller.phase == .waiting }
}

@MainActor @Test func aReplyWithNothingToSayEndsTheWait() async throws {
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("run the tests")
  #expect(rig.controller.phase == .waitingReply)
  rig.controller.replyUpdated("```\nmake test\n```", final: true, id: "m1")
  #expect(rig.controller.phase == .waiting)
  #expect(rig.audio.played.isEmpty)
}

@MainActor @Test func theWaitEndsWhenTheReplyDoes() async throws {
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("run the tests")
  rig.controller.replyEnded()
  #expect(rig.controller.phase == .waiting)
}

@MainActor @Test func aSecondMessageIsAReplyOfItsOwnAndIsSpokenAfterTheFirst() async throws {
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("hello")
  rig.controller.replyUpdated("Hi there, friend.", final: true, id: "m1")
  try await rig.eventually { rig.audio.played.count == 1 }
  rig.controller.replyUpdated("Anything else you need?", final: true, id: "m2")
  rig.audio.finishPlayback()
  try await rig.eventually { rig.audio.played.count == 2 }
  rig.audio.finishPlayback()
  try await rig.eventually { rig.controller.phase == .waiting }
  #expect(await rig.voice.spoken == ["Hi there, friend.", "Anything else you need?"])
}

@MainActor @Test func aReplyWaitsWhileThePersonTalksAndIsSpokenAfter() async throws {
  let rig = Rig()
  try await rig.controller.start()
  await rig.hear(3, level: 0.9)
  rig.controller.replyUpdated("Good morning, how are you?", final: true, id: "x")
  try await Task.sleep(for: .milliseconds(30))
  #expect(rig.audio.played.isEmpty)
  rig.controller.interrupt()
  try await rig.eventually { rig.audio.played.count == 1 }
}

// MARK: - Talking over

@MainActor @Test func talkingOverTheReplyStopsItAndSaysWhatWasHeard() async throws {
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("tell me a story")
  rig.controller.replyUpdated("Hi there, friend. How can I help you today?", final: false, id: "m1")
  try await rig.eventually { rig.audio.played.count == 1 }
  // Half of the first sentence has been heard.
  rig.audio.playbackPosition = 0.5 * Double(rig.audio.played[0].samples.count) / 16_000

  await rig.hear(11, level: 0.9)
  #expect(rig.controller.phase == .probing)
  rig.lastRecognizer?.onEvent?(.partial("wait stop"))
  #expect(rig.audio.stoppedPlayback == 1)
  #expect(rig.cancelled == 1)
  #expect(rig.controller.phase == .hearing)
  #expect(rig.controller.partialTranscript == "wait stop")

  await rig.hear(11, level: 0.1)
  try await rig.eventually { rig.controller.phase == .finalizing }
  rig.lastRecognizer?.onEvent?(.final("wait stop that"))
  #expect(rig.turns.last == VoiceTurn(text: "wait stop that", heardOfInterruptedReply: "Hi"))
  // The late words of the cancelled reply are not spoken as the next one.
  rig.controller.replyUpdated(reply, final: true, id: "m1")
  #expect(rig.audio.played.count == 1)
  // The next reply is.
  rig.controller.replyUpdated("Of course, here it is.", final: true, id: "m2")
  try await rig.eventually { rig.audio.played.count == 2 }
}

@MainActor @Test func theAvatarsOwnVoiceIsNotAnInterruptionAndIsNotShown() async throws {
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("hello")
  rig.controller.replyUpdated("Hi there, friend. How", final: false, id: "m1")
  try await rig.eventually { rig.audio.played.count == 1 }
  await rig.hear(11, level: 0.9)
  rig.lastRecognizer?.onEvent?(.partial("hi there friend"))
  #expect(rig.audio.stoppedPlayback == 0)
  #expect(rig.controller.partialTranscript == "hello")
  await rig.hear(11, level: 0.1)
  rig.lastRecognizer?.onEvent?(.final("hi there friend"))
  #expect(rig.controller.phase == .playing)
  #expect(rig.lastRecognizer?.finished == true)
}

@MainActor @Test func talkingWhileTheAgentThinksTakesTheTurnOnlyWithWords() async throws {
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("run the tests")
  await rig.hear(3, level: 0.9)
  await rig.hear(20, level: 0.1)
  #expect(rig.cancelled == 0)
  #expect(rig.controller.phase == .waitingReply)
  await rig.hear(11, level: 0.9)
  #expect(rig.controller.phase == .probingWait)
  rig.lastRecognizer?.onEvent?(.partial("actually wait"))
  #expect(rig.cancelled == 1)
  #expect(rig.controller.phase == .hearing)
}

@MainActor @Test func aKeyPressStopsTheAvatar() async throws {
  let rig = Rig()
  try await rig.startSpeaking()
  rig.controller.interrupt()
  #expect(rig.audio.stoppedPlayback == 1)
  #expect(rig.cancelled == 1)
  #expect(rig.controller.phase == .waiting)
}

// MARK: - Starting, stopping, failing

@MainActor @Test func theLoopDoesNotStartWithoutPermission() async throws {
  let recognizer = FakeRecognizer()
  recognizer.authorization = .denied("Allow the microphone.")
  let rig = Rig(recognizer: recognizer)
  await #expect(throws: VoiceLoopError.notAuthorized("Allow the microphone.")) {
    try await rig.controller.start()
  }
  #expect(!rig.audio.started)
  #expect(rig.controller.phase == .off)
}

@MainActor @Test func aStopWhileThePermissionIsAskedKeepsTheMicrophoneClosed() async throws {
  let recognizer = FakeRecognizer()
  recognizer.held = true
  let rig = Rig(recognizer: recognizer)
  let starting = Task { try await rig.controller.start() }
  try await rig.eventually { recognizer.gates.count == 1 }
  rig.controller.stop()
  recognizer.release()
  try await starting.value
  #expect(rig.audio.startCount == 0)
  #expect(rig.controller.phase == .off)
}

@MainActor @Test func aSecondStartWhileTheFirstWaitsOpensTheMicrophoneOnce() async throws {
  let recognizer = FakeRecognizer()
  recognizer.held = true
  let rig = Rig(recognizer: recognizer)
  let first = Task { try await rig.controller.start() }
  try await rig.eventually { recognizer.gates.count == 1 }
  let second = Task { try await rig.controller.start() }
  try await rig.eventually { recognizer.gates.count == 2 }
  recognizer.release()
  try await first.value
  try await second.value
  try await rig.eventually { rig.audio.startCount >= 1 }
  try await Task.sleep(for: .milliseconds(30))
  #expect(rig.audio.startCount == 1)
}

@MainActor @Test func aMicrophoneThatCannotStartLeavesTheLoopOff() async throws {
  let rig = Rig()
  rig.audio.startError = VoiceAudioError.noInput
  await #expect(throws: VoiceAudioError.noInput) { try await rig.controller.start() }
  #expect(rig.controller.phase == .off)
}

@MainActor @Test func theLoopEndsWithTheReasonWhenTheAudioStopsUnderIt() async throws {
  let rig = Rig()
  try await rig.startSpeaking()
  rig.audio.onFailure?("The audio device changed.")
  #expect(rig.controller.phase == .off)
  #expect(rig.controller.failure == "The audio device changed.")
  #expect(!rig.audio.started)
  #expect(rig.cancelled == 1)
}

@MainActor @Test func aMicrophoneThatDeliversNothingEndsTheLoop() async throws {
  var options = VoiceLoopOptions()
  options.watchdog = .milliseconds(60)
  let rig = Rig(options: options)
  try await rig.controller.start()
  try await rig.eventually { rig.controller.phase == .off }
  #expect(rig.controller.failure == "The microphone stopped delivering audio.")
}

@MainActor @Test func stoppingEndsTheMicrophoneAndWhatIsUnderWay() async throws {
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("hello")
  rig.controller.stop()
  #expect(!rig.audio.started)
  #expect(rig.cancelled == 1)
  #expect(rig.controller.state == .idle)
}

@MainActor @Test func aControllerDroppedWhileRunningClosesTheMicrophone() async throws {
  let audio = FakeAudio()
  weak var weakController: VoiceLoopController?
  do {
    let controller = VoiceLoopController(
      audio: audio, recognizer: { FakeRecognizer() }, synthesizer: FakeVoice(),
      activity: ScriptedActivity())
    weakController = controller
    try await controller.start()
    #expect(audio.started)
  }
  for _ in 0..<200 where audio.started { try await Task.sleep(for: .milliseconds(5)) }
  #expect(weakController == nil)
  #expect(!audio.started)
}

// MARK: - The front end, voice activity and loudness

@Test func theFrontEndKeepsTheLastEightSecondsInOrderAndTimesTheFrames() async {
  let front = VoiceFrontEnd(detector: EnergyVoiceActivity())
  let total = 10 * VoiceFrames.sampleRate
  let ramp = (0..<total).map { Float($0 % 1_000) / 1_000 }
  var last: VoiceFrontEnd.Frame?
  var count = 0
  for start in stride(from: 0, to: total, by: 3_000) {
    let result = await front.process(Array(ramp[start..<min(start + 3_000, total)]))
    count += result.frames.count
    last = result.frames.last ?? last
  }
  #expect(count == total / VoiceFrames.size)
  #expect(last?.at == (count * VoiceFrames.size) * 1_000 / VoiceFrames.sampleRate)
  let window = await front.window()
  #expect(window.count == VoiceFrames.turnWindow)
  let end = count * VoiceFrames.size
  #expect(window == Array(ramp[(end - VoiceFrames.turnWindow)..<end]))
}

@Test func theEnergyDetectorTellsAVoiceFromTheRoom() {
  let detector = EnergyVoiceActivity()
  let room = [Float](repeating: 0.002, count: VoiceFrames.size).enumerated().map { index, level in
    index.isMultiple(of: 2) ? level : -level
  }
  let voice = (0..<VoiceFrames.size).map { 0.3 * sinf(Float($0) * 0.2) }
  for _ in 0..<20 { #expect(detector.probability(of: room) == 0) }
  #expect(detector.probability(of: voice) == 1)
  #expect(detector.probability(of: room) == 0)
  detector.reset()
  #expect(detector.probability(of: [Float](repeating: 0, count: VoiceFrames.size)) == 0)
}

@Test func theFramesAreThirtyTwoMilliseconds() {
  #expect(VoiceFrames.frameMs == VoiceTurnConfig().frameMs)
  #expect(VoiceFrames.size * 1_000 / VoiceFrames.sampleRate == VoiceFrames.frameMs)
}

@Test func theEnvelopeIsTheLoudnessOfTheAudioAtAPosition() {
  let samples = [Float](repeating: 0, count: 1_600) + [Float](repeating: 0.5, count: 1_600)
  #expect(SpeechEnvelope.level(of: samples, sampleRate: 16_000, at: 0) == 0)
  #expect(SpeechEnvelope.level(of: samples, sampleRate: 16_000, at: 0.11) == 0.5)
  #expect(SpeechEnvelope.level(of: samples, sampleRate: 16_000, at: 5) == 0)
  #expect(SpeechEnvelope.level(of: samples, sampleRate: 16_000, at: -1) == 0)
  #expect(SpeechEnvelope.level(of: [], sampleRate: 16_000, at: 0) == 0)
}

@MainActor @Test func onlyARecognizerTheLoopStartsMayJoinTheMicrophone() async throws {
  let session = VoiceAudioSession()
  #expect(!session.allowsJoin)
  session.listening(from: 0) { #expect(session.allowsJoin) }
  #expect(!session.allowsJoin)
  struct Refused: Error {}
  #expect(throws: Refused.self) { try session.listening(from: 0) { throw Refused() } }
  #expect(!session.allowsJoin)
}
