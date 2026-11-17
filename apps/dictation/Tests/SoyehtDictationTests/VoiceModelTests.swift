import CoreML
import Foundation
import SoyehtClient
import Testing
import os

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
  let window = SmartTurnEndOfTurn.window
  #expect(window == 8 * 16_000)
  let short = SmartTurnEndOfTurn.padded([1, 2, 3])
  #expect(short.count == window)
  #expect(short.prefix(window - 3).allSatisfy { $0 == 0 })
  #expect(Array(short.suffix(3)) == [1, 2, 3])
  let long = (0..<(window + 10)).map { Float($0) }
  let kept = SmartTurnEndOfTurn.padded(long)
  #expect(kept.count == window)
  #expect(kept.first == 10 && kept.last == Float(window + 9))
}

@Test func twoInstallsOfOneModelAtOnceLoadItOnce() async throws {
  let base = FileManager.default.temporaryDirectory.appendingPathComponent(
    "voice-holder-\(UUID().uuidString)")
  defer { try? FileManager.default.removeItem(at: base) }
  let loads = OSAllocatedUnfairLock(initialState: 0)
  let holder = LocalModelHolder<Int>(
    location: ModelCacheLocation(modelId: "test/model", base: base)
  ) { _, directory, _ in
    loads.withLock { $0 += 1 }
    try await Task.sleep(for: .milliseconds(100))
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    return 7
  }
  async let first: Void = holder.install { _ in }
  async let second: Void = holder.install { _ in }
  try await first
  try await second
  #expect(loads.withLock { $0 } == 1)
  #expect(holder.current == 7)
  #expect(holder.isInstalled)
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
  #expect(throws: VoiceModelError.frameSize(expected: 512, got: 100)) {
    try SileroVoiceActivity.requireFrame([Float](repeating: 0, count: 100))
  }
  try SileroVoiceActivity.requireFrame([Float](repeating: 0, count: 512))
}

@Test func aNumberThatIsNotAProbabilityIsAnErrorNotAnAnswer() throws {
  #expect(try VoiceModelError.probability(0.5) == 0.5)
  for bad in [Double.nan, -0.1, 1.5, .infinity] {
    #expect(throws: VoiceModelError.self) { try VoiceModelError.probability(bad) }
  }
}

@Test func theProbabilityIsReadInTheTypeTheModelWritesItIn() throws {
  let half = try MLMultiArray(shape: [1, 1], dataType: .float16)
  half.dataPointer.assumingMemoryBound(to: Float16.self)[0] = 0.5
  #expect(try SileroVoiceActivity.value(of: half) == 0.5)
  let single = try MLMultiArray(shape: [1, 1], dataType: .float32)
  single.dataPointer.assumingMemoryBound(to: Float.self)[0] = 0.25
  #expect(try SileroVoiceActivity.value(of: single) == 0.25)
  let integer = try MLMultiArray(shape: [1, 1], dataType: .int32)
  #expect(throws: VoiceModelError.self) { try SileroVoiceActivity.value(of: integer) }
}
