import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/screenplay.json`, which the desktop client passes as well.
private func vector() throws -> JsonValue { try ContractVector.load("screenplay") }

private func kind(_ value: JsonValue) throws -> ScreenplayElementKind {
  try #require(value.string.flatMap(ScreenplayElementKind.init(rawValue:)))
}

private func ranges(_ value: JsonValue) throws -> [NSRange] {
  try (value.array ?? []).map { pair in
    let parts = try #require(pair.array)
    return NSRange(
      location: try #require(parts[0].int), length: try #require(parts[1].int))
  }
}

@Test func tabAndReturnMoveToTheElementTheVectorSays() throws {
  let cases = ContractVector.cases(try vector(), "advance")
  #expect(!cases.isEmpty)
  for item in cases {
    let next = ScreenplayInteraction.nextKind(
      after: try kind(item["kind"]), tab: item["tab"].bool ?? false)
    #expect(next == (try kind(item["next"])), "\(item["kind"].string ?? "")")
  }
}

@Test func anElementIsSetAsTheLineTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "formatting") {
    let line = ScreenplayInteraction.formattedLine(
      try #require(item["source"].string), as: try kind(item["kind"]))
    #expect(line == item["line"].string, "\(item["name"].string ?? "")")
  }
}

@Test func tabAndReturnMakeTheEditTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "advanceEdit") {
    let name = item["name"].string ?? ""
    let source = try #require(item["source"].string)
    let cursor = try #require(item["cursor"].int)
    let element = try kind(item["kind"])
    let edit = try #require(
      ScreenplayInteraction.advanceEdit(
        source: source, cursor: cursor, kind: element, tab: item["tab"].bool ?? false),
      "\(name)")
    #expect(edit.range.location == item["range"]["location"].int, "\(name)")
    #expect(edit.range.length == item["range"]["length"].int, "\(name)")
    #expect(edit.replacement == item["replacement"].string, "\(name)")
  }
}

@Test func aLineSitsOnThePageAsTheVectorSays() throws {
  for item in ContractVector.cases(try vector(), "lineStyle") {
    let name = item["name"].string ?? ""
    let style = ScreenplayLineStyle(
      kind: try kind(item["kind"]), line: try #require(item["line"].string))
    let alignment: ScreenplayLineStyle.Alignment =
      switch item["alignment"].string {
      case "center": .center
      case "left": .left
      case "right": .right
      default: .natural
      }
    #expect(style.alignment == alignment, "\(name)")
    #expect((style.headIndent > 0) == (item["inset"].bool ?? false), "\(name)")
    #expect(style.bold == (item["bold"].bool ?? false), "\(name)")
    #expect(style.secondary == (item["secondary"].bool ?? false), "\(name)")
    #expect(style.hiddenRanges == (try ranges(item["hidden"])), "\(name)")
  }
}
