import Foundation
import Testing

@testable import SoyehtClient

// MARK: - Fakes

@MainActor private final class FakeAudio: VoiceAudio {
  var started = false
  var replays: [Int] = []
  var played: [SynthesizedSpeech] = []
  var stoppedPlayback = 0
  var playbackPosition = 0.0
  private var waiting: CheckedContinuation<Void, Never>?

  func start(onSamples: @escaping @Sendable ([Float]) -> Void) throws { started = true }
  func stop() { started = false }
  func replay(from milliseconds: Int) { replays.append(milliseconds) }

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
  var onEvent: (@MainActor (SpeechEvent) -> Void)?
  var finished = false
  var cancelled = false

  func authorize() async -> DictationAuthorization { authorization }

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
  func probability(of frame: [Float]) -> Double { level }
  func reset() {}
}

private struct ScriptedEndOfTurn: EndOfTurnDetecting {
  let complete: Bool
  func isComplete(turn audio: [Float]) async -> Bool { complete }
}

/// Everything a test of the loop needs, wired.
@MainActor private final class Rig {
  let audio = FakeAudio()
  let voice = FakeVoice()
  let activity = ScriptedActivity()
  var turns: [VoiceTurn] = []
  var cancelled = 0
  let controller: VoiceLoopController

  init(
    endOfTurn: any EndOfTurnDetecting = SilenceEndOfTurn(),
    options: VoiceLoopOptions = VoiceLoopOptions(), recognizer: FakeRecognizer? = nil
  ) {
    let made = RecognizerBox()
    controller = VoiceLoopController(
      audio: audio,
      recognizer: {
        let next = recognizer ?? FakeRecognizer()
        made.all.append(next)
        return next
      }, synthesizer: voice, activity: activity, endOfTurn: endOfTurn, options: options)
    box = made
    controller.onTurn = { [unowned self] in turns.append($0) }
    controller.onCancelReply = { [unowned self] in cancelled += 1 }
  }

  private var box: RecognizerBox
  var lastRecognizer: FakeRecognizer? { box.all.last }

  /// `frames` frames of audio the voice-activity model reads as `level`.
  func hear(_ frames: Int, level: Double) {
    activity.level = level
    controller.feed([Float](repeating: 0, count: frames * VoiceFrames.size))
  }

  func eventually(_ condition: @MainActor () -> Bool) async throws {
    for _ in 0..<400 where !condition() { try await Task.sleep(for: .milliseconds(5)) }
    #expect(condition())
  }

  /// A turn answered, the first sentence of the reply playing.
  func startSpeaking() async throws {
    try await controller.start()
    try await say("hello")
    controller.replyUpdated("Hi there, friend. How", final: false)
    try await eventually { self.audio.played.count == 1 }
  }

  /// A whole turn up to the person's words being sent: speech, a pause, the recognizer's final.
  func say(_ words: String) async throws {
    hear(3, level: 0.9)
    hear(8, level: 0.9)
    lastRecognizer?.onEvent?(.partial(words))
    hear(11, level: 0.1)
    try await eventually { self.controller.phase == .finalizing }
    lastRecognizer?.onEvent?(.final(words))
  }
}

@MainActor private final class RecognizerBox {
  var all: [FakeRecognizer] = []
}

private let reply = "Hi there, friend. How can I help you today?"

// MARK: - Tests

@MainActor @Test func aSpokenTurnReachesTheAgentAndItsReplyIsSpokenInOrder() async throws {
  let rig = Rig()
  try await rig.controller.start()
  #expect(rig.audio.started)
  #expect(rig.controller.phase == .waiting)

  rig.hear(3, level: 0.9)
  #expect(rig.controller.phase == .hearing)
  #expect(rig.audio.replays == [0])
  rig.hear(8, level: 0.9)
  rig.lastRecognizer?.onEvent?(.partial("what time"))
  #expect(rig.controller.partialTranscript == "what time")
  rig.hear(11, level: 0.1)
  try await rig.eventually { rig.controller.phase == .finalizing }
  #expect(rig.lastRecognizer?.finished == true)
  rig.lastRecognizer?.onEvent?(.final("what time is it"))
  #expect(rig.turns == [VoiceTurn(text: "what time is it")])
  #expect(rig.controller.state == .thinking)

  rig.controller.replyUpdated("Hi there, friend. How can", final: false)
  try await rig.eventually { rig.audio.played.count == 1 }
  #expect(rig.controller.state == .speaking)
  #expect(rig.controller.currentSentence == "Hi there, friend.")
  rig.audio.finishPlayback()
  rig.controller.replyUpdated(reply, final: true)
  try await rig.eventually { rig.audio.played.count == 2 }
  rig.audio.finishPlayback()
  try await rig.eventually { rig.controller.phase == .waiting }
  #expect(await rig.voice.spoken == ["Hi there, friend.", "How can I help you today?"])
  #expect(rig.controller.currentSpeech == nil)
}

@MainActor @Test func theLastPartialTextIsUsedWhenTheRecognizerNeverFinishes() async throws {
  var options = VoiceLoopOptions()
  options.finalTranscriptTimeout = .milliseconds(30)
  let rig = Rig(options: options)
  try await rig.controller.start()
  rig.hear(3, level: 0.9)
  rig.lastRecognizer?.onEvent?(.partial("open the file"))
  rig.hear(11, level: 0.1)
  try await rig.eventually { rig.turns.count == 1 }
  #expect(rig.turns == [VoiceTurn(text: "open the file")])
}

@MainActor @Test func aTurnTheModelCallsUnfinishedWaitsForMoreSpeech() async throws {
  let rig = Rig(endOfTurn: ScriptedEndOfTurn(complete: false))
  try await rig.controller.start()
  rig.hear(3, level: 0.9)
  rig.hear(11, level: 0.1)
  try await rig.eventually { rig.controller.phase == .deciding }
  try await Task.sleep(for: .milliseconds(50))
  #expect(rig.controller.phase == .deciding)
  rig.hear(60, level: 0.1)
  #expect(rig.controller.phase == .finalizing)
}

@MainActor @Test func nothingIsSentWhenTheRecognizerHeardNothing() async throws {
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("   ")
  #expect(rig.turns.isEmpty)
  #expect(rig.controller.phase == .waiting)
}

@MainActor @Test func talkingOverTheReplyStopsItAndSaysWhatWasHeard() async throws {
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("tell me a story")
  rig.controller.replyUpdated("Hi there, friend. How can I help you today?", final: false)
  try await rig.eventually { rig.audio.played.count == 1 }
  // Half of the first sentence has been heard.
  rig.audio.playbackPosition = 0.5 * Double(rig.audio.played[0].samples.count) / 16_000

  rig.hear(11, level: 0.9)
  #expect(rig.controller.phase == .probing)
  rig.lastRecognizer?.onEvent?(.partial("wait stop"))
  #expect(rig.audio.stoppedPlayback == 1)
  #expect(rig.cancelled == 1)
  #expect(rig.controller.phase == .hearing)

  rig.hear(11, level: 0.1)
  try await rig.eventually { rig.controller.phase == .finalizing }
  rig.lastRecognizer?.onEvent?(.final("wait stop that"))
  #expect(rig.turns.last == VoiceTurn(text: "wait stop that", heardOfInterruptedReply: "Hi"))
  // The old reply is gone: what it still sends is not spoken.
  rig.controller.replyUpdated(reply, final: true)
  #expect(rig.audio.played.count == 1)
}

@MainActor @Test func theAvatarsOwnVoiceIsNotAnInterruption() async throws {
  let rig = Rig()
  try await rig.startSpeaking()
  rig.hear(11, level: 0.9)
  rig.lastRecognizer?.onEvent?(.partial("hi there"))
  #expect(rig.audio.stoppedPlayback == 0)
  rig.hear(11, level: 0.1)
  #expect(rig.controller.phase == .playing)
  #expect(rig.lastRecognizer?.cancelled == true)
}

@MainActor @Test func aReplyNobodyAskedForIsNotSpoken() async throws {
  let rig = Rig()
  try await rig.controller.start()
  rig.controller.replyUpdated(reply, final: true)
  try await Task.sleep(for: .milliseconds(30))
  #expect(rig.audio.played.isEmpty)
}

@MainActor @Test func aKeyPressStopsTheAvatar() async throws {
  let rig = Rig()
  try await rig.startSpeaking()
  rig.controller.interrupt()
  #expect(rig.audio.stoppedPlayback == 1)
  #expect(rig.cancelled == 1)
  #expect(rig.controller.phase == .waiting)
}

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

@MainActor @Test func stoppingEndsTheMicrophoneAndWhatIsUnderWay() async throws {
  let rig = Rig()
  try await rig.controller.start()
  try await rig.say("hello")
  rig.controller.stop()
  #expect(!rig.audio.started)
  #expect(rig.cancelled == 1)
  #expect(rig.controller.state == .idle)
}

// MARK: - Voice activity and loudness

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
}

@Test func theEnvelopeIsTheLoudnessOfTheAudioAtAPosition() {
  let samples = [Float](repeating: 0, count: 1_600) + [Float](repeating: 0.5, count: 1_600)
  #expect(SpeechEnvelope.level(of: samples, sampleRate: 16_000, at: 0) == 0)
  #expect(SpeechEnvelope.level(of: samples, sampleRate: 16_000, at: 0.11) == 0.5)
  #expect(SpeechEnvelope.level(of: samples, sampleRate: 16_000, at: 5) == 0)
  #expect(SpeechEnvelope.level(of: samples, sampleRate: 16_000, at: -1) == 0)
  #expect(SpeechEnvelope.level(of: [], sampleRate: 16_000, at: 0) == 0)
}
