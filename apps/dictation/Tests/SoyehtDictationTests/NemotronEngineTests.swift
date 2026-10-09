import Foundation
import SoyehtClient
import Testing

@testable import SoyehtDictation

@Test func theCatalogueListsAppleFirstAsTheFallbackThenNemotron() {
  let engines = DictationCatalogue.registry.engines
  #expect(engines.map(\.id) == ["apple", "nemotron"])
  #expect(DictationCatalogue.registry.fallback.model == nil)
}

@Test func nemotronDeclaresWhatPreferencesShows() {
  let engine = NemotronEngine.engine
  #expect(engine.platforms == Set(DictationPlatform.allCases))
  #expect(engine.usesVocabulary)
  #expect(engine.languages.prefix(2).map(\.id) == ["system", "auto"])
  #expect(engine.languages.map(\.id).contains("pt-BR"))
  #expect(engine.model?.licenseName == "OpenMDW-1.1")
  #expect(engine.model?.sizeDescription == "about 642 MB")
  #expect(engine.requiresInstall)
}

@MainActor @Test func appleIsTheDefaultEngineAndNemotronIsOptIn() {
  let defaults = UserDefaults(suiteName: "nemotron-tests-\(UUID().uuidString)") ?? .standard
  let store = DictationEngineStore(
    registry: DictationCatalogue.registry, defaults: defaults, platform: .mac)
  #expect(store.selected.id == "apple")
  #expect(store.active.id == "apple")
  // Choosing it without installing it selects nothing and downloads nothing.
  store.select("nemotron")
  #expect(store.selected.id == "apple")
}

@Test func nemotronNeedsAppleSiliconAndSaysSo() {
  #if arch(arm64)
    #expect(NemotronEngine.availability() == .available)
  #else
    #expect(NemotronEngine.availability() == .unavailable("Needs Apple silicon"))
  #endif
}

@MainActor @Test func nemotronRefusesToListenBeforeItsModelIsLoaded() {
  let recognizer = NemotronRecognizer()
  #expect(throws: NemotronRecognizer.Failure.self) {
    try recognizer.start(SpeechSessionOptions()) { _ in }
  }
}
