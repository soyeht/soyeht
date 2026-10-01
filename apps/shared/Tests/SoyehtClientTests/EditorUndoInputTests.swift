import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func point(_ position: TextPosition?) -> [UInt32]? {
  position.map { [$0.line, $0.column] }
}

@Test func verbatimPasteReplacesEveryCursorWithoutAutoClosingAndLeavesCaretsAfter() throws {
  let text = "let 😀 = a\nlet b = (c"
  let ranges = [NSRange(location: 9, length: 1), NSRange(location: 19, length: 2)]
  let plan = try #require(
    EditorEditing.verbatimPlan(
      text: text, viewportStartLine: 12, ranges: ranges.reversed(),
      strings: ["(x", "{\ny}"].reversed()))
  #expect(EditorEditing.applying(plan.replacements, to: text) == "let 😀 = (x\nlet b = {\ny}")
  #expect(plan.selections == [NSRange(location: 11, length: 0), NSRange(location: 24, length: 0)])
  #expect(plan.edits.count == 2)
  #expect(point(plan.edits[0].range.start) == [12, 9])
  #expect(point(plan.edits[1].range.start) == [13, 8])
  #expect(point(plan.edits[1].range.end) == [13, 10])
  #expect(plan.edits[1].text == "{\ny}")
}

@Test func verbatimCutRemovesTheSelectionAndRejectsMismatchedOrOverlappingInput() throws {
  let cut = try #require(
    EditorEditing.verbatimPlan(
      text: "alpha\nbeta", viewportStartLine: 0,
      ranges: [NSRange(location: 3, length: 4)], strings: [""]))
  #expect(EditorEditing.applying(cut.replacements, to: "alpha\nbeta") == "alpeta")
  #expect(point(cut.edits.first?.range.end) == [1, 1])
  #expect(cut.selections == [NSRange(location: 3, length: 0)])

  #expect(
    EditorEditing.verbatimPlan(
      text: "alpha", viewportStartLine: 0, ranges: [NSRange(location: 0, length: 1)],
      strings: []) == nil)
  #expect(
    EditorEditing.verbatimPlan(
      text: "alpha", viewportStartLine: 0,
      ranges: [NSRange(location: 0, length: 3), NSRange(location: 2, length: 1)],
      strings: ["x", "y"]) == nil)
  #expect(
    EditorEditing.verbatimPlan(
      text: "alpha", viewportStartLine: 0, ranges: [NSRange(location: 4, length: 3)],
      strings: ["x"]) == nil)
}

@Test func undoLandingKeepsALoadedPageAndCentersAnUnloadedLine() {
  #expect(EditorViewportPaging.landingStart(line: 40, loadedFrom: 0, loadedTo: 200) == 0)
  #expect(
    EditorViewportPaging.landingStart(line: 5_000, loadedFrom: 0, loadedTo: 2_000)
      == 5_000 - EditorViewportPaging.pageLines / 4)
  #expect(EditorViewportPaging.landingStart(line: 3, loadedFrom: 900, loadedTo: 2_900) == 0)
}

@Test func vimUndoIsANormalModeEffectAndHistoryDirectionsNameTheirMethods() {
  var vim = EditorVimMachine()
  let undo = vim.handle(
    .character("u"), text: "one", viewportStartLine: 0,
    selections: [NSRange(location: 1, length: 0)], language: .rust)
  #expect(undo.handled)
  #expect(undo.effect == .undo)
  #expect(undo.plan == nil)
  #expect(EditorHistoryDirection.undo.method == "file.undo")
  #expect(EditorHistoryDirection.redo.method == "file.redo")
}
