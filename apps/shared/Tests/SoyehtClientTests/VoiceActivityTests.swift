import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func theFramesAreThirtyTwoMilliseconds() {
  #expect(VoiceFrames.frameMs == VoiceTurnConfig().frameMs)
  #expect(VoiceFrames.size * 1_000 / VoiceFrames.sampleRate == VoiceFrames.frameMs)
  #expect(VoiceFrames.size == 512)
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
