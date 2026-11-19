import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient
@testable import SoyehtVoice

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
  #expect(VoiceFrames.size == 512)
}

@Test func theEnvelopeIsTheLoudnessOfTheAudioAtAPosition() {
  let samples = [Float](repeating: 0, count: 1_600) + [Float](repeating: 0.5, count: 1_600)
  #expect(SpeechEnvelope.level(of: samples, sampleRate: 16_000, at: 0) == 0)
  #expect(SpeechEnvelope.level(of: samples, sampleRate: 16_000, at: 0.11) == 0.5)
  #expect(SpeechEnvelope.level(of: samples, sampleRate: 16_000, at: 5) == 0)
  #expect(SpeechEnvelope.level(of: samples, sampleRate: 16_000, at: -1) == 0)
  #expect(SpeechEnvelope.level(of: [], sampleRate: 16_000, at: 0) == 0)
}

@Test func aSilenceEndOfTurnAlwaysSaysFinished() async throws {
  #expect(try await SilenceEndOfTurn().isComplete(turn: []))
}

@Test func theRangesOfThePolicyAreCodePointsOfScriptsAndSymbols() {
  let blankless = ScalarRanges(policy: ClientPolicy.voiceScriptsWithoutBlanks)
  #expect(blankless.contains("日") && blankless.contains("の") && blankless.contains("ก"))
  #expect(!blankless.contains("a") && !blankless.contains("é"))
  let symbols = ScalarRanges(policy: ClientPolicy.voicePictographs)
  #expect(
    symbols.contains("\u{1F389}") && symbols.contains("\u{2192}") && symbols.contains("\u{20E3}"))
  #expect(!symbols.contains("a") && !symbols.contains("."))
}

/// A model that cannot answer says so; nothing makes its failure a silence.
private struct Failing: VoiceActivityDetecting, EndOfTurnDetecting {
  struct Broken: Error {}

  func probability(of frame: [Float]) throws -> Double { throw Broken() }
  func reset() {}
  func isComplete(turn audio: [Float]) async throws -> Bool { throw Broken() }
}

@Test func aModelThatCannotAnswerThrows() async {
  #expect(throws: Failing.Broken.self) { try Failing().probability(of: []) }
  await #expect(throws: Failing.Broken.self) { try await Failing().isComplete(turn: []) }
}
