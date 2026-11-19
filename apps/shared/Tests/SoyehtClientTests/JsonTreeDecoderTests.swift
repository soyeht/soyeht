import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The tree a frame is parsed into is read into types directly. These tests hold it to what the
/// text route (write the tree out, parse it again) read.

private func viaText<T: Codable>(_ type: T.Type, _ tree: JsonValue) throws -> JsonValue {
  try .encoded(JSONDecoder().decode(type, from: JSONEncoder().encode(tree)))
}

private func viaTree<T: Codable>(_ type: T.Type, _ tree: JsonValue) throws -> JsonValue {
  try .encoded(tree.decoded(type))
}

@Test func aValueOfTheWrongKindOrAMissingFieldIsRefused() throws {
  let tree = try JsonValue.encoded(PaneLayoutFixture.pane("p", workspace: "w", mode: .agent))
  guard case .object(var fields) = tree else {
    Issue.record("a pane is an object")
    return
  }
  fields["cols"] = .number(80.5)
  #expect(throws: DecodingError.self) { try JsonValue.object(fields).decoded(Pane.self) }
  fields["cols"] = nil
  #expect(throws: DecodingError.self) { try JsonValue.object(fields).decoded(Pane.self) }
  #expect(throws: DecodingError.self) { try JsonValue.string("p").decoded(Pane.self) }
}

private struct Foundationish: Codable, Equatable {
  var bytes: Data
  var link: URL
  var when: Date
}

@Test func theTypesJSONDecoderReadsInAWayOfItsOwnAreReadTheSameFromTheTree() throws {
  let text = #"{"bytes":"aGVsbG8=","link":"https://example.com/a?b=1","when":1234.5}"#
  let tree = try JSONDecoder().decode(JsonValue.self, from: Data(text.utf8))
  #expect(
    try tree.decoded(Foundationish.self)
      == JSONDecoder().decode(Foundationish.self, from: Data(text.utf8)))
}
