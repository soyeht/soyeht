import Foundation
import Testing

@testable import SoyehtClient

private let viewport = WritingViewportText(source: "one\ntwo\n\nfour", firstLine: 10)

@Test func aViewportNamesTheDocumentPositionOfEveryOffset() throws {
  let start = try #require(viewport.position(utf16Offset: 0))
  #expect(start.line == 10)
  let inSecond = try #require(viewport.position(utf16Offset: 5))
  #expect(inSecond.line == 11)
  #expect(inSecond.column == 1)
  #expect(viewport.position(utf16Offset: 400) == nil)
  #expect(viewport.line(utf16Offset: 8) == 12)
  #expect(viewport.line(utf16Offset: 9) == 13)
  #expect(viewport.line(utf16Offset: 400) == 10)
}

@Test func aSelectionIsKeptInsideTheViewportText() {
  #expect(viewport.clamped(NSRange(location: 2, length: 4)) == NSRange(location: 2, length: 4))
  #expect(viewport.clamped(NSRange(location: 11, length: 99)) == NSRange(location: 11, length: 2))
  #expect(viewport.clamped(NSRange(location: 99, length: 1)) == NSRange(location: 0, length: 0))
  #expect(
    viewport.clamped(NSRange(location: NSNotFound, length: 0)) == NSRange(location: 0, length: 0))
}

@Test func theLocationAndRangeOfALineFollowTheViewportStart() {
  #expect(viewport.utf16Location(ofLine: 10) == 0)
  #expect(viewport.utf16Location(ofLine: 12) == 8)
  #expect(viewport.utf16Location(ofLine: 13) == 9)
  #expect(viewport.utf16Location(ofLine: 9) == nil)
  #expect(viewport.utf16Location(ofLine: 14) == nil)

  #expect(viewport.range(fromLine: 11, throughLine: 11) == NSRange(location: 4, length: 3))
  #expect(viewport.range(fromLine: 10, throughLine: 11) == NSRange(location: 0, length: 7))
  #expect(viewport.range(fromLine: 12, throughLine: 99) == NSRange(location: 8, length: 5))
  #expect(viewport.range(fromLine: 9, throughLine: 10) == nil)
  #expect(viewport.range(fromLine: 11, throughLine: 10) == nil)
  #expect(viewport.range(fromLine: 14, throughLine: 14) == nil)
}

@Test func theContentOfALineLeavesItsBreaksOut() {
  let crlf = WritingViewportText(source: "alpha\r\nbeta\n", firstLine: 0)
  #expect(crlf.contentRange(atLocation: 2) == NSRange(location: 0, length: 5))
  #expect(crlf.contentRange(atLocation: 8) == NSRange(location: 7, length: 4))
  #expect(crlf.contentRange(atLocation: 12) == NSRange(location: 12, length: 0))
  #expect(crlf.contentRange(atLocation: 400) == NSRange(location: 12, length: 0))
}

@Test func focusModeDimsEveryNonEmptyLineButTheActiveOne() {
  #expect(
    viewport.inactiveLineRanges(activeLine: 11) == [
      NSRange(location: 0, length: 3), NSRange(location: 9, length: 4),
    ])
  #expect(
    viewport.inactiveLineRanges(activeLine: 99) == [
      NSRange(location: 0, length: 3), NSRange(location: 4, length: 3),
      NSRange(location: 9, length: 4),
    ])
  #expect(viewport.inactiveLineRanges(activeLine: 3).isEmpty)
}

@Test func replacingARangeEditsTheTextAndNamesTheWireEdit() throws {
  var text = viewport
  let replaced = text.replace(NSRange(location: 4, length: 3), with: "2\n2")
  let change = try #require(replaced)
  #expect(text.source == "one\n2\n2\n\nfour")
  #expect(change.cursor == 7)
  #expect(change.edit.text == "2\n2")
  #expect(change.edit.range.start.line == 11)
  #expect(change.edit.range.end.line == 11)
  #expect(change.edit.range.end.column == 3)

  var unchanged = viewport
  #expect(unchanged.replace(NSRange(location: 50, length: 1), with: "x") == nil)
  #expect(unchanged == viewport)
}
