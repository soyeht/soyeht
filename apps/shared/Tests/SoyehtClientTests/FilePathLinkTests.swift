import SoyehtClient
import Testing

@Test func filePathLinksCoverAgentTerminalAndCiLocations() {
  let text =
    "Agent: Sources/App.swift:12:7\n" + "Terminal: engine/src/main.rs:4\n"
    + "CI: ./apps/mac/Tests/WindowTests.swift:81:3"
  let references: [FilePathReference] = FilePathLinks.segments(in: text).compactMap { segment in
    guard case .reference(_, let reference) = segment else { return nil }
    return reference
  }

  #expect(
    references == [
      FilePathReference(path: "Sources/App.swift", line: 11, column: 6),
      FilePathReference(path: "engine/src/main.rs", line: 3),
      FilePathReference(path: "apps/mac/Tests/WindowTests.swift", line: 80, column: 2),
    ])
}

@Test func filePathLinksRejectUnsafeAmbiguousAndInvalidLocations() {
  let rejected = [
    "/private/tmp/file.swift:2:1",
    "../outside/file.swift:2:1",
    "https://example.test/file.swift:2:1",
    "file.swift:0:1",
    "file.swift:2:0",
    "identifier:2",
    "file.swift:4294967296",
  ]

  for value in rejected {
    #expect(FilePathReference.parse(value) == nil)
    #expect(FilePathLinks.segments(in: value) == [.plain(value)])
  }
}

@Test func filePathLinksPreserveEveryUnlinkedByteAndMultipleMatches() {
  let text = "See (src/one.rs:7), then README.md:18."
  let segments = FilePathLinks.segments(in: text)

  let rebuilt = segments.map { segment in
    switch segment {
    case .plain(let value), .reference(let value, _): value
    }
  }.joined()
  #expect(rebuilt == text)
  #expect(segments.count == 5)
}
