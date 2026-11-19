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

@Test func aTypedValueReadFromTheTreeIsTheOneTheTextRouteReads() throws {
  let pane = PaneLayoutFixture.pane("p", workspace: "w", mode: .agent)
  var paneTree = try JsonValue.encoded(pane)
  // A whole number written with a fraction still fits an integer field.
  if case .object(var fields) = paneTree {
    fields["cols"] = .number(120.0)
    paneTree = .object(fields)
  }
  let update = try JsonValue.encoded(
    PaneUpdateEvent.v0(
      AgentMessageUpdate(
        eventId: "7", at: "2026-11-18T00:00:00Z", paneId: "p", kind: .agent_message_chunk,
        generation: 1, seq: "9", localId: "m", text: "héllo \u{1F600}", complete: false)))

  #expect(try viaTree(Pane.self, paneTree) == viaText(Pane.self, paneTree))
  #expect(try viaTree([Pane].self, .array([paneTree])) == viaText([Pane].self, .array([paneTree])))
  #expect(try viaTree(PaneUpdateEvent.self, update) == viaText(PaneUpdateEvent.self, update))
  #expect(try paneTree.decoded(Pane.self).cols == 120)
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

@Test func aJsonFrameIsParsedToItsTree() async throws {
  let json = Frame(kind: FrameKind.json, payload: Data(#"{"jsonrpc":"2.0","n":[1,2.5,null]}"#.utf8))
  guard case .message(let tree) = try await IncomingFrame.parse(json) else {
    Issue.record("a JSON frame becomes a tree")
    return
  }
  #expect(tree["n"] == .array([.integer(1), .number(2.5), .null]))
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
