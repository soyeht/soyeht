import Foundation
import SoyehtClient
import Testing

@Test func webLinkRoutingDefaultsLocalLinksInsideAndExternalLinksToTheSystem() throws {
  #expect(
    WebLinkRouting.destination(for: "http://localhost:5173/app")
      == .soyeht(try #require(URL(string: "http://localhost:5173/app"))))
  #expect(
    WebLinkRouting.destination(for: "http://127.0.0.1:8291")
      == .soyeht(try #require(URL(string: "http://127.0.0.1:8291"))))
  #expect(
    WebLinkRouting.destination(for: "https://example.test/docs")
      == .system(try #require(URL(string: "https://example.test/docs"))))
  #expect(
    WebLinkRouting.destination(
      for: "https://example.test/docs", openExternalLinksInSoyeht: true)
      == .soyeht(try #require(URL(string: "https://example.test/docs"))))
}

@Test func webLinkRoutingRejectsUnsafeSchemesCredentialsAndMalformedValues() {
  for value in [
    "file:///private/tmp/secret", "javascript:alert(1)", "https://name:secret@example.test",
    "https://", "not a URL",
  ] {
    #expect(WebLinkRouting.destination(for: value) == nil)
  }
}

@Test func webLinksPreserveBoundariesAndStripSentencePunctuation() {
  let text = "Open https://example.test/docs, then http://localhost:5173/app."
  let links = WebLinkRouting.links(in: text)
  #expect(links.map(\.label) == ["https://example.test/docs", "http://localhost:5173/app"])
  #expect(links.map { String(text[$0.range]) } == links.map(\.label))
}

@MainActor @Test func webLinkPreferencePersistsTheExternalOverride() {
  let store = MemoryPreferenceStore()
  let preferences = WebLinkPreferences(preferences: store)
  #expect(!preferences.openExternalLinksInSoyeht)
  preferences.setOpenExternalLinksInSoyeht(true)
  #expect(WebLinkPreferences(preferences: store).openExternalLinksInSoyeht)
}
