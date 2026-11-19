import AVFoundation
import Foundation
import Testing
import os

@testable import SoyehtClient
@testable import SoyehtVoice

// MARK: - Fakes

@MainActor private final class FakeAudio: VoiceAudio {
  var onFailure: (@MainActor (String) -> Void)?
  var startError: (any Error)?
  var startCount = 0
  var started = false
  var foreignCaptureActive = false
  var foreignLeftAt = 0
  private var onSamples: (@Sendable ([Float]) -> Void)?
  var replays: [Int] = []
  var played: [SynthesizedSpeech] = []
  var stoppedPlayback = 0
  var playbackPosition = 0.0
  private var waiting: CheckedContinuation<Void, Never>?

  func start(onSamples: @escaping @Sendable ([Float]) -> Void) throws {
    if let startError { throw startError }
    startCount += 1
    started = true
    self.onSamples = onSamples
  }

  /// A buffer from the microphone, as the audio thread would deliver it.
  func deliver() { onSamples?([Float](repeating: 0, count: 16)) }

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
  private let delay: Duration

  init(delay: Duration = .zero) { self.delay = delay }

  func synthesize(_ text: String, options: SpeechSynthesisOptions) async throws
    -> SynthesizedSpeech
  {
    spoken.append(text)
    if delay > .zero { try await Task.sleep(for: delay) }
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
  let voice: FakeVoice
  let activity = ScriptedActivity()
  var turns: [VoiceTurn] = []
  var cancelled = 0
  let controller: VoiceLoopController
  private let box = RecognizerBox()

  init(
    voiceDelay: Duration = .zero, endOfTurn: any EndOfTurnDetecting = SilenceEndOfTurn(),
    options requested: VoiceLoopOptions = VoiceLoopOptions(), recognizer: FakeRecognizer? = nil
  ) {
    // The fake never delivers a buffer unless a test says so: the real 10 s start deadline, in real
    // time, must not end the loop of a test on a loaded machine.
    var options = requested
    if options.firstBuffer == VoiceLoopOptions().firstBuffer {
      options.firstBuffer = .seconds(3_600)
    }
    voice = FakeVoice(delay: voiceDelay)
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

  /// A turn is sent, its reply comes while the person starts to talk (so it is held), and then they
  /// only hum along: the probe ends and the held reply is free to speak.
  func holdAReplyWhileThePersonHumsAlong(_ markdown: String) async throws {
    try await controller.start()
    try await say("run the tests")
    controller.replyUpdated(markdown, final: true, id: "m1")
    await hear(11, level: 0.9)
    #expect(controller.phase == .probingWait)
    try await Task.sleep(for: .milliseconds(300))
    #expect(audio.played.isEmpty)
    lastRecognizer?.onEvent?(.partial("mhm"))
    await hear(11, level: 0.1)
    lastRecognizer?.onEvent?(.final("mhm"))
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

@MainActor @Test func aRecognizerThatFailsInAPauseStillGivesTheTurnItsWords() async throws {
  let rig = Rig()
  try await rig.controller.start()
  await rig.hear(3, level: 0.9)
  rig.lastRecognizer?.onEvent?(.partial("open the file"))
  await rig.hear(4, level: 0.1)
  rig.lastRecognizer?.onEvent?(.failed("the recognizer stopped"))
  await rig.hear(11, level: 0.1)
  try await rig.eventually { rig.turns.count == 1 }
  #expect(rig.turns == [VoiceTurn(text: "open the file")])
  #expect(rig.controller.failure == "the recognizer stopped")
}

@MainActor @Test func aRecognizerThatFailsWhileThePersonTalksIsReplacedAtOnce() async throws {
  let rig = Rig()
  try await rig.controller.start()
  await rig.hear(3, level: 0.9)
  let first = rig.lastRecognizer
  first?.onEvent?(.partial("open the"))
  first?.onEvent?(.failed("the recognizer stopped"))
  #expect(rig.lastRecognizer !== first)
  // The audio the first one heard is not given to the second.
  #expect((rig.audio.replays.last ?? 0) >= rig.controller.now)
  rig.lastRecognizer?.onEvent?(.partial("file"))
  await rig.hear(11, level: 0.1)
  try await rig.eventually { rig.controller.phase == .finalizing }
  rig.lastRecognizer?.onEvent?(.final("file"))
  #expect(rig.turns == [VoiceTurn(text: "open the file")])
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

@MainActor @Test func aSentenceIsHeldWhileThePersonTalksOverTheWaitAndSpokenWhenTheyStop()
  async throws
{
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("run the tests")
  await rig.hear(11, level: 0.9)
  #expect(rig.controller.phase == .probingWait)
  rig.controller.replyUpdated("Good morning, how are you?", final: true, id: "m1")
  try await Task.sleep(for: .milliseconds(30))
  #expect(rig.audio.played.isEmpty)
  // The person is only humming along: the probe ends and the avatar speaks.
  rig.lastRecognizer?.onEvent?(.partial("mhm"))
  await rig.hear(11, level: 0.1)
  rig.lastRecognizer?.onEvent?(.final("mhm"))
  try await rig.eventually { rig.audio.played.count == 1 }
}

@MainActor @Test func theLoopHearsSilenceWhileADictationSharesTheMicrophone() async throws {
  let rig = Rig()
  try await rig.controller.start()
  rig.audio.foreignCaptureActive = true
  await rig.hear(8, level: 0.9)
  #expect(rig.controller.phase == .waiting)
  rig.audio.foreignCaptureActive = false
  await rig.hear(4, level: 0.9)
  #expect(rig.controller.phase == .hearing)
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
  // The late words of the cancelled reply are not spoken as the next one, and do not end the wait
  // for the agent's answer to the new turn.
  rig.controller.replyUpdated(reply, final: true, id: "m1")
  #expect(rig.audio.played.count == 1)
  #expect(rig.controller.phase == .waitingReply)
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
  // The agent's work is not cancelled: the audio failed, the person did not interrupt.
  #expect(rig.cancelled == 0)
}

@MainActor @Test func aMicrophoneThatDeliversNothingEndsTheLoop() async throws {
  var options = VoiceLoopOptions()
  options.watchdog = .milliseconds(60)
  let rig = Rig(options: options)
  try await rig.controller.start()
  rig.audio.deliver()
  try await rig.eventually { rig.controller.phase == .off }
  #expect(rig.controller.failure == "The microphone stopped delivering audio.")
  #expect(rig.cancelled == 0)
}

@MainActor @Test func theWatchdogWaitsForTheFirstBuffer() async throws {
  var options = VoiceLoopOptions()
  options.watchdog = .milliseconds(30)
  let rig = Rig(options: options)
  try await rig.controller.start()
  try await Task.sleep(for: .milliseconds(150))
  #expect(rig.controller.phase == .waiting)
  rig.controller.stop()
}

@MainActor @Test func aMicrophoneThatNeverDeliversAnythingEndsTheLoopAtTheStartDeadline()
  async throws
{
  var options = VoiceLoopOptions()
  options.watchdog = .milliseconds(30)
  options.firstBuffer = .milliseconds(120)
  let rig = Rig(options: options)
  try await rig.controller.start()
  try await rig.eventually { rig.controller.phase == .off }
  #expect(rig.controller.failure == "The microphone did not deliver any audio.")
  #expect(rig.cancelled == 0)
}

@MainActor @Test func aSlowVoiceThatFinishesAfterThePersonBeganToTalkIsHeldAndNotMadeAgain()
  async throws
{
  let rig = Rig(voiceDelay: .milliseconds(200))
  try await rig.holdAReplyWhileThePersonHumsAlong("Good morning, how are you?")
  try await rig.eventually { rig.audio.played.count == 1 }
  #expect(await rig.voice.spoken == ["Good morning, how are you?"])
}

@MainActor @Test func aRecognizerOfTheLoopNeverStartsFromBeforeADictationLeft() async throws {
  let rig = Rig()
  try await rig.controller.start()
  rig.audio.foreignLeftAt = 100_000
  await rig.hear(3, level: 0.9)
  #expect(rig.audio.replays.last == 100_000)
}

@MainActor @Test func stoppingEndsTheMicrophoneAndWhatIsUnderWay() async throws {
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("hello")
  rig.controller.stop()
  #expect(!rig.audio.started)
  // Turning the voice off leaves the agent to its work.
  #expect(rig.cancelled == 0)
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

@Test func aModelThatFailedIsTrustedAgainAtTheNextStream() async {
  struct Flaky: VoiceActivityDetecting {
    struct Broken: Error {}

    func probability(of frame: [Float]) throws -> Double { throw Broken() }
    func reset() {}
  }
  let front = VoiceFrontEnd(detector: Flaky())
  let frame = [Float](repeating: 0, count: VoiceFrames.size)
  let first = await front.process(frame)
  #expect(first.failure != nil)
  #expect((await front.process(frame)).failure == nil)
  await front.reset()
  #expect((await front.process(frame)).failure != nil)
}

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

@MainActor @Test func aCaptureTheLoopDidNotStartIsForeignAndTheLoopHearsSilenceMeanwhile() throws {
  let session = VoiceAudioSession()
  #expect(!session.foreignCaptureActive)
  let capture = MicrophoneCapture()
  var loops = 0
  session.listening(from: 0) {
    loops = session.attach({ _ in }, owner: capture)
    #expect(!session.foreignCaptureActive)
  }
  let dictation = session.attach({ _ in }, owner: capture)
  #expect(session.foreignCaptureActive)
  session.detach(dictation)
  #expect(!session.foreignCaptureActive)
  session.detach(loops)
}

@MainActor @Test func aDictationDroppedWhileJoinedLeavesTheLoopHearing() throws {
  let session = VoiceAudioSession()
  var capture: MicrophoneCapture? = MicrophoneCapture()
  capture?.join(session, sink: { _ in })
  #expect(session.foreignCaptureActive)
  capture = nil
  #expect(!session.foreignCaptureActive)
  #expect(MicrophoneCapture.ownEngineCaptures.isEmpty)
}

@MainActor @Test func aDictationThatLeavesIsRememberedSoNoRecognizerStartsFromBeforeIt() throws {
  let session = VoiceAudioSession()
  let capture = MicrophoneCapture()
  capture.join(session, sink: { _ in })
  capture.stop()
  #expect(!session.foreignCaptureActive)
}

@Test func aCaptureKeepsTheFormatOfItsFirstBuffer() throws {
  let wide = try #require(AVAudioFormat(standardFormatWithSampleRate: 48_000, channels: 1))
  let narrow = try #require(AVAudioFormat(standardFormatWithSampleRate: 16_000, channels: 1))
  func buffer(_ format: AVAudioFormat, frames: AVAudioFrameCount) throws -> AVAudioPCMBuffer {
    let made = try #require(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: frames))
    made.frameLength = frames
    return made
  }
  let keeper = FormatKeeper { _ in }
  #expect(keeper.steady(try buffer(wide, frames: 480))?.format == wide)
  #expect(keeper.steady(try buffer(wide, frames: 480))?.format == wide)
  let converted = try #require(keeper.steady(try buffer(narrow, frames: 160)))
  #expect(converted.format == wide)
  #expect(converted.frameLength > 0)
}

@Test func aBufferThatCannotBeConvertedEndsTheCaptureOnceAndIsNotPassedOn() throws {
  struct Unreadable: Error {}
  let wide = try #require(AVAudioFormat(standardFormatWithSampleRate: 48_000, channels: 1))
  let narrow = try #require(AVAudioFormat(standardFormatWithSampleRate: 16_000, channels: 1))
  func buffer(_ format: AVAudioFormat) throws -> AVAudioPCMBuffer {
    let made = try #require(AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 160))
    made.frameLength = 160
    return made
  }
  let told = OSAllocatedUnfairLock(initialState: 0)
  let keeper = FormatKeeper(
    convert: { _, _ in throw Unreadable() }, failed: { _ in told.withLock { $0 += 1 } })
  #expect(keeper.steady(try buffer(wide)) != nil)
  #expect(keeper.steady(try buffer(narrow)) == nil)
  #expect(keeper.steady(try buffer(narrow)) == nil)
  #expect(keeper.steady(try buffer(wide)) == nil)
  #expect(told.withLock { $0 } == 1)
}

@MainActor @Test func aCaptureThatCannotKeepItsFormatEndsWithTheReason() throws {
  struct Unreadable: Error {}
  let session = VoiceAudioSession()
  let capture = MicrophoneCapture()
  var reasons: [String] = []
  capture.onEnded = { reasons.append(String(describing: $0)) }
  // Nothing is held: there is nothing to end.
  capture.fail(Unreadable())
  #expect(reasons.isEmpty)
  capture.join(session, sink: { _ in })
  #expect(session.foreignCaptureActive)
  capture.fail(Unreadable())
  #expect(reasons.count == 1)
  #expect(!session.foreignCaptureActive)
}

@MainActor @Test func aSessionPassesEventsOnOnlyWhileItLasts() {
  let session = RecognitionSession()
  var heard: [SpeechEvent] = []
  let events = session.begin { heard.append($0) }
  events(.partial("open"))
  events(.failed("The audio went away."))
  session.end()
  // The system's own cancellation comes after the real reason and does not replace it.
  events(.failed("The operation was cancelled."))
  #expect(heard == [.partial("open"), .failed("The audio went away.")])
  let next = session.begin { heard.append($0) }
  events(.final("old"))
  next(.final("new"))
  #expect(heard.last == .final("new"))
  #expect(!heard.contains(.final("old")))
}

@MainActor @Test func aSentenceAfterAHeldOneIsMadeOnceToo() async throws {
  let rig = Rig(voiceDelay: .milliseconds(200))
  try await rig.holdAReplyWhileThePersonHumsAlong(
    "First we look at the code. Then we run the tests. Last we report the result.")
  try await rig.eventually { rig.audio.played.count == 1 }
  rig.audio.finishPlayback()
  try await rig.eventually { rig.audio.played.count == 2 }
  rig.audio.finishPlayback()
  try await rig.eventually { rig.audio.played.count == 3 }
  #expect(await rig.voice.spoken.count == 3)
}
