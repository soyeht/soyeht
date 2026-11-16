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
