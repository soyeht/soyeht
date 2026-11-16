import Foundation
import SoyehtClient
import Testing

@testable import SoyehtDictation

// None of these tests downloads a model or loads one: the models are not on the machine that runs
// them, and a test that needed one would fail rather than fetch it.

@Test func theVoiceLoopModelsListTheOnesThatNeedNoDownloadFirst() {
  #expect(VoiceModelCatalogue.activity.entries.map(\.id) == ["energy", "silero"])
  #expect(VoiceModelCatalogue.endOfTurn.entries.map(\.id) == ["silence", "smart-turn"])
  #expect(VoiceModelCatalogue.activity.fallback.model == nil)
  #expect(VoiceModelCatalogue.endOfTurn.fallback.model == nil)
}

private func expectLicenseAndInstall<Made>(_ provider: VoiceModelProvider<Made>) throws {
  let model = try #require(provider.model)
  #expect(model.commercialUse)
  #expect(provider.license.name == model.licenseName)
  #expect(provider.license.url == model.licenseURL)
}

@Test func theLocalModelsDeclareTheirLicenseAndAskForAnInstall() throws {
  try expectLicenseAndInstall(SileroEngine.provider)
  try expectLicenseAndInstall(SmartTurnEngine.provider)
  #expect(SileroEngine.provider.license.name == "MIT")
  #expect(SmartTurnEngine.provider.license.name == "BSD-2-Clause")
}

@Test func aModelThatIsNotLoadedGivesNoDetectorSoTheLoopUsesTheFallback() {
  #expect(SileroEngine.provider.make() == nil)
  #expect(SmartTurnEngine.provider.make() == nil)
  #expect(VoiceModelCatalogue.activity.fallback.make() != nil)
}

@Test func theTurnModelReadsTheLastEightSecondsAndLeavesTheFrontZero() {
  #expect(SmartTurnEndOfTurn.window == 8 * 16_000)
}

@Test func theLocalVoiceModelsFollowTheLocalMemoryRule() {
  let rule = LocalModelAvailability.appleSiliconWithEnoughMemory()
  #expect(SileroEngine.provider.availability() == rule)
  #expect(SmartTurnEngine.provider.availability() == rule)
}

@Test func theSileroFrameIsTheFrameOfTheLoop() {
  #expect(SileroVoiceActivity.chunkSize == VoiceFrames.size)
}

@Test func theDownloadsArePinnedToACommitAndAChecksum() {
  for model in [PinnedModel.sileroVAD, PinnedModel.smartTurn] {
    #expect(model.revision.count == 40 && model.revision.allSatisfy(\.isHexDigit))
    #expect(!model.files.isEmpty)
    for file in model.files {
      #expect(file.sha256.count == 64 && file.sha256.allSatisfy(\.isHexDigit))
      #expect(file.size > 0)
    }
  }
}

@Test func anInstalledModelIsCheckedNotFetched() async throws {
  let directory = FileManager.default.temporaryDirectory.appendingPathComponent(
    "voice-pinned-\(UUID().uuidString)")
  defer { try? FileManager.default.removeItem(at: directory) }
  // Nothing is there: an offline load fails, naming the file, and never reaches for the network.
  await #expect(throws: PinnedDownload.Failure.self) {
    try await PinnedDownload.fetch(.smartTurn, to: directory, offline: true) { _ in }
  }
  for file in PinnedModel.smartTurn.files {
    let target = directory.appendingPathComponent(file.path)
    try FileManager.default.createDirectory(
      at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
    try Data(count: file.size).write(to: target)
  }
  try await PinnedDownload.fetch(.smartTurn, to: directory, offline: true) { _ in }
  // A file of the wrong size is not the pinned one.
  let weights = directory.appendingPathComponent("smart_turn.mlmodelc/weights/weight.bin")
  try Data(count: 3).write(to: weights)
  await #expect(throws: PinnedDownload.Failure.self) {
    try await PinnedDownload.fetch(.smartTurn, to: directory, offline: true) { _ in }
  }
}

@Test func aFrameOfTheWrongSizeIsAnErrorNotASilence() throws {
  struct Failing: VoiceActivityDetecting {
    func probability(of frame: [Float]) throws -> Double {
      guard frame.count == VoiceFrames.size else {
        throw VoiceModelError.frameSize(expected: VoiceFrames.size, got: frame.count)
      }
      return 0
    }

    func reset() {}
  }
  #expect(throws: VoiceModelError.frameSize(expected: 512, got: 100)) {
    try Failing().probability(of: [Float](repeating: 0, count: 100))
  }
}
