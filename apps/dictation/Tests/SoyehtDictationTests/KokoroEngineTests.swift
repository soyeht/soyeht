import Foundation
import SoyehtClient
import Testing

@testable import SoyehtDictation

// None of these tests downloads the model, loads it or speaks: the model is not on the machine
// that runs them, and a test that needed it would fail rather than fetch it.

@Test func theVoiceCatalogueListsTheSystemVoicesFirstAsTheFallbackThenKokoro() {
  let providers = VoiceCatalogue.synthesis.providers
  #expect(providers.map(\.id) == ["apple", "kokoro"])
  #expect(VoiceCatalogue.synthesis.fallback.model == nil)
  #expect(VoiceCatalogue.synthesis.fallback.cloud == nil)
}

@Test func aProviderTheAppAddsComesAfterTheBuiltInOnes() {
  let cloud = SpeechSynthesisProvider.cloud(
    CloudSpeechConfiguration(
      id: "cloud.example", name: "Example", baseURL: URL(string: "https://speech.example.com/v1")!,
      transcriptionModel: "listen-1", speechModel: "speak-1", voices: ["river"]),
    vault: EmptyVault())
  #expect(
    VoiceCatalogue.synthesis(adding: [cloud]).providers.map(\.id) == [
      "apple", "kokoro", "cloud.example",
    ])
  #expect(DictationCatalogue.registry(adding: []).engines.map(\.id) == ["apple", "nemotron"])
}

private struct EmptyVault: ProviderKeyVault {
  func key(for provider: String) throws -> String? { nil }
  func save(_ key: String, for provider: String) throws {}
  func forget(provider: String) throws {}
}

@Test func kokoroDeclaresWhatTheVoiceScreenShows() throws {
  let provider = KokoroEngine.provider
  #expect(provider.platforms == Set(DictationPlatform.allCases))
  #expect(provider.cloud == nil)
  #expect(provider.requiresInstall)
  let model = try #require(provider.model)
  #expect(model.licenseName == "Apache-2.0")
  #expect(model.commercialUse)
  #expect(model.sizeDescription == "about 333 MB")
}

@Test func kokoroOffersEnglishVoicesOnlyWithTheDefaultFirst() {
  let voices = KokoroEngine.provider.voices()
  #expect(voices.first?.id == "af_heart")
  #expect(voices.count == 28)
  #expect(Set(voices.map(\.language)) == ["en-US", "en-GB"])
  #expect(voices.allSatisfy { $0.id.hasPrefix("a") || $0.id.hasPrefix("b") })
  #expect(voices.first { $0.id == "bm_george" }?.name == "George")
  #expect(Set(voices.map(\.id)).count == voices.count)
}

@Test func kokoroNeedsAppleSiliconAndSaysSo() {
  #if arch(arm64)
    #expect(KokoroEngine.availability() == .available)
  #else
    #expect(KokoroEngine.availability() == .unavailable("Needs Apple silicon"))
  #endif
}

@Test func kokoroRefusesToSpeakBeforeItsModelIsLoaded() async {
  let synthesizer = KokoroEngine.provider.make()
  await #expect(throws: KokoroEngine.KokoroSpeechSynthesizer.Failure.self) {
    try await synthesizer.synthesize("Hello there.", options: SpeechSynthesisOptions())
  }
}

// MARK: The pieces a text is cut into

@Test func aShortTextIsOnePiece() {
  #expect(SpeechTextChunks.split("Hello there", maxCharacters: 60) == ["Hello there"])
  #expect(SpeechTextChunks.split("  \n ", maxCharacters: 60).isEmpty)
}

@Test func eachSentenceStartsANewPiece() {
  #expect(
    SpeechTextChunks.split("Yes. I can do that! Can you?", maxCharacters: 60)
      == ["Yes.", "I can do that!", "Can you?"])
}

@Test func aLongSentenceBreaksBetweenWordsAndNoPieceIsOverTheLimit() {
  let sentence = Array(repeating: "alpha beta gamma", count: 12).joined(separator: " ") + "."
  let pieces = SpeechTextChunks.split(sentence, maxCharacters: 60)
  #expect(pieces.count > 1)
  #expect(pieces.allSatisfy { $0.count <= 60 })
  #expect(pieces.joined(separator: " ") == sentence)
}

@Test func aWordLongerThanAPieceIsCutRatherThanLost() {
  let word = String(repeating: "x", count: 150)
  let pieces = SpeechTextChunks.split("start \(word) end", maxCharacters: 60)
  #expect(pieces.allSatisfy { $0.count <= 60 })
  #expect(pieces.joined().replacingOccurrences(of: " ", with: "") == "start" + word + "end")
}

@Test func aSlowerVoiceGetsSmallerPiecesWithinBounds() {
  #expect(SpeechTextChunks.limit(forSpeed: 1) == 60)
  #expect(SpeechTextChunks.limit(forSpeed: 0.1) == 30)
  #expect(SpeechTextChunks.limit(forSpeed: 10) == 100)
}
