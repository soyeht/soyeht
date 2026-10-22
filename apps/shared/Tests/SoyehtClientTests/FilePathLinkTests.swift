import SoyehtProtocol
import Testing

@testable import SoyehtClient

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

/// The cases of `contracts/vectors/file-paths.json`, which the desktop client passes as well.
@Test func filePathLinksPassTheSharedVectorCases() throws {
  let cases = ContractVector.cases(try ContractVector.load("file-paths"), "cases")
  #expect(cases.count > 10)
  for item in cases {
    let text = try #require(item["text"].string)
    let found: [(String, FilePathReference)] = FilePathLinks.segments(in: text).compactMap {
      guard case .reference(let label, let reference) = $0 else { return nil }
      return (label, reference)
    }
    let expected: [(String, FilePathReference)] = try (item["references"].array ?? []).map {
      let line = try #require($0["line"].int)
      return (
        try #require($0["label"].string),
        FilePathReference(
          path: try #require($0["path"].string), line: UInt32(line),
          column: $0["column"].int.map { UInt32($0) })
      )
    }
    #expect(found.map(\.0) == expected.map(\.0), "\(item["name"].string ?? "")")
    #expect(found.map(\.1) == expected.map(\.1), "\(item["name"].string ?? "")")
  }
}
