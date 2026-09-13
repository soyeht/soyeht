import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func editorInputMapsViewportUtf16RangesAndMultipleCursorsAtomically() throws {
  let text = "let 😀 = one\nlet 😀 = one"
  let ranges = [NSRange(location: 9, length: 3), NSRange(location: 22, length: 3)]
  let plan = try #require(
    EditorEditing.insertionPlan(
      text: text, viewportStartLine: 40, ranges: ranges, inserted: "value", language: .swift))

  #expect(plan.edits.count == 2)
  #expect(plan.edits[0].range.start.line == 40)
  #expect(plan.edits[0].range.start.column == 9)
  #expect(plan.edits[1].range.start.line == 41)
  #expect(plan.edits[1].range.start.column == 9)
  #expect(EditorEditing.applying(plan.replacements, to: text) == "let 😀 = value\nlet 😀 = value")
}

@Test func editorInputAutoClosesIndentsAndDeletesWholeScalars() throws {
  let pair = try #require(
    EditorEditing.insertionPlan(
      text: "call", viewportStartLine: 0, ranges: [NSRange(location: 4, length: 0)],
      inserted: "(", language: .rust))
  #expect(EditorEditing.applying(pair.replacements, to: "call") == "call()")
  #expect(pair.selections == [NSRange(location: 5, length: 0)])

  let newline = try #require(
    EditorEditing.insertionPlan(
      text: "    if ready {", viewportStartLine: 0,
      ranges: [NSRange(location: 14, length: 0)], inserted: "\n", language: .rust))
  #expect(
    EditorEditing.applying(newline.replacements, to: "    if ready {") == "    if ready {\n        "
  )

  let emoji = try #require(
    EditorEditing.deletionPlan(
      text: "a😀b", viewportStartLine: 0, ranges: [NSRange(location: 3, length: 0)],
      backward: true, language: .swift))
  #expect(EditorEditing.applying(emoji.replacements, to: "a😀b") == "ab")

  let emptyPair = try #require(
    EditorEditing.deletionPlan(
      text: "call()", viewportStartLine: 0, ranges: [NSRange(location: 5, length: 0)],
      backward: true, language: .swift))
  #expect(EditorEditing.applying(emptyPair.replacements, to: "call()") == "call")
}

@Test func editorInputTogglesCommentsAndFindsTheNextOccurrence() throws {
  let text = "  first\n  second\nfirst"
  let commented = try #require(
    EditorEditing.toggleCommentPlan(
      text: text, viewportStartLine: 7, ranges: [NSRange(location: 0, length: 16)],
      language: .swift))
  let result = EditorEditing.applying(commented.replacements, to: text)
  #expect(result == "  // first\n  // second\nfirst")
  #expect(commented.edits.map(\.range.start.line) == [7, 8])

  let selected = NSRange(location: 4, length: 3)
  #expect(
    EditorEditing.nextOccurrence(text: "one two one two", ranges: [selected])
      == NSRange(location: 12, length: 3))
  #expect(EditorEditing.wordRange(text: "one value", at: 6) == NSRange(location: 4, length: 5))
}
