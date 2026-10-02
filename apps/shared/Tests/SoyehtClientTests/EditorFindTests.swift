import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func page(_ lines: [String], from: UInt32 = 0, total: UInt32? = nil) -> FileReadResult {
  let count = UInt32(lines.count)
  let file = FileDocument(
    fileId: "file-1", paneId: "pane-1", workspaceId: "workspace-1", path: "main.swift",
    language: .swift, revision: "3", diskRevision: "1", lineCount: total ?? from + count,
    byteCount: "100", dirty: true, conflicted: false, watching: true)
  return FileReadResult(
    file: file, source: .buffer, fromLine: from, toLine: from + count,
    lines: lines.enumerated().map {
      EditorLine(line: from + UInt32($0.offset), text: $0.element, highlights: [])
    })
}

private func target(_ line: UInt32, _ column: UInt32, end: (UInt32, UInt32)? = nil)
  -> EditorNavigationTarget
{
  EditorNavigationTarget(
    id: UUID().uuidString, fileId: "file-1", position: TextPosition(line: line, column: column),
    selectionEnd: end.map { TextPosition(line: $0.0, column: $0.1) })
}

/// After Cmd+Z the engine names where the restored edit ends; the caret must
/// land there, never past the end of a line the undo shortened.
@Test func undoLandingIsClampedToTheRestoredLineAndTheDocument() {
  let restored = page(["fn main() {", "let x = 1", "}"])
  #expect(
    EditorViewportPaging.landing(target(1, 9), in: restored) == NSRange(location: 21, length: 0))
  // A stale column (Col 17 on a 9-character line) clamps to the line end.
  #expect(
    EditorViewportPaging.landing(target(1, 16), in: restored) == NSRange(location: 21, length: 0))
  // A line past the end of the document clamps to the document end.
  #expect(
    EditorViewportPaging.landing(target(9, 0), in: restored) == NSRange(location: 23, length: 0))
  // A line outside a page that does not reach the document end is not loaded.
  let middle = page(["a", "b"], from: 100, total: 5_000)
  #expect(EditorViewportPaging.landing(target(4_000, 0), in: middle) == nil)
  #expect(EditorViewportPaging.localOffset(TextPosition(line: 101, column: 0), in: middle) == 2)
}

@Test func findLandingSelectsTheMatchInsideTheLoadedPage() {
  let loaded = page(["let 😀 = needle", "needle"], from: 40, total: 900)
  #expect(
    EditorViewportPaging.landing(target(40, 9, end: (40, 15)), in: loaded)
      == NSRange(location: 9, length: 6))
  #expect(
    EditorViewportPaging.landing(target(41, 0, end: (41, 6)), in: loaded)
      == NSRange(location: 16, length: 6))
}

@Test func findOptionsBuildTheEngineQuery() {
  let query = EditorFindOptions(caseSensitive: true, wholeWord: false, regex: true).query("a+")
  #expect(query.text == "a+")
  #expect(query.caseSensitive && query.regex && !query.wholeWord)
}

@MainActor @Test func findControllerReportsCountsAndSeedsTheQueryFromTheSelection() {
  let client = EngineClient(socket: "/tmp/sn-editor-find/engine.sock")
  let find = EditorFindController(client: client, fileId: "file-1")
  #expect(find.status.isEmpty)
  let selection = SoyehtProtocol.TextRange(
    start: TextPosition(line: 3, column: 2), end: TextPosition(line: 3, column: 8))
  find.present(replace: true, selection: selection, selectedText: "needle")
  #expect(find.isPresented && find.showsReplace)
  #expect(find.query == "needle")
  #expect(find.fieldFocusRequest == 1)
  find.present(replace: false, selection: nil, selectedText: "two\nlines")
  #expect(find.query == "needle")
  #expect(!find.showsReplace)
  find.close()
  #expect(!find.isPresented)
  #expect(find.editorFocusRequest == 1)
  #expect(find.result == nil && find.status.isEmpty)
}

@MainActor @Test func findStatusReadsOneBasedOrdinalOfTotal() {
  #expect(EditorFindController.status(query: "x", result: nil, problem: nil).isEmpty)
  let range = SoyehtProtocol.TextRange(
    start: TextPosition(line: 0, column: 0), end: TextPosition(line: 0, column: 1))
  let found = FileFindResult(
    revision: "1", total: 41, current: FileFindMatch(index: 2, range: range), visible: [])
  #expect(EditorFindController.status(query: "x", result: found, problem: nil) == "3 of 41")
  let none = FileFindResult(revision: "1", total: 0, visible: [])
  #expect(EditorFindController.status(query: "x", result: none, problem: nil) == "No results")
  #expect(EditorFindController.status(query: "", result: found, problem: nil).isEmpty)
  #expect(
    EditorFindController.status(query: "(", result: nil, problem: "bad pattern") == "bad pattern")
}

#if os(macOS)
  import AppKit

  /// A click in the empty area right of a short line must reach the text view
  /// and land at that line's end, as in every Mac text editor.
  @MainActor @Test func clickPastAShortLineHitsTheTextViewAndLandsAtTheLineEnd() throws {
    let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 600, height: 300))
    let textView = NSTextView(frame: scroll.bounds)
    scroll.documentView = textView
    EditorTextLayout.configure(textView)
    textView.textContainerInset = NSSize(width: 10, height: 10)
    textView.string = "short\nanother short line\nx"
    let layout = try #require(textView.layoutManager)
    let container = try #require(textView.textContainer)
    layout.ensureLayout(for: container)
    textView.sizeToFit()
    #expect(textView.frame.width < scroll.contentSize.width, "AppKit shrinks to the text")

    EditorTextLayout.fit(textView, visible: scroll.contentSize, documentHeight: 80)
    textView.sizeToFit()
    #expect(textView.frame.width >= scroll.contentSize.width)
    let point = NSPoint(x: 500, y: 12)
    #expect(textView.frame.contains(point))
    let local = NSPoint(
      x: point.x - textView.textContainerOrigin.x, y: point.y - textView.textContainerOrigin.y)
    var fraction: CGFloat = 0
    let glyph = layout.glyphIndex(
      for: local, in: container, fractionOfDistanceThroughGlyph: &fraction)
    #expect(layout.characterIndexForGlyph(at: glyph) == 5)

    EditorTextLayout.fit(
      textView, visible: NSSize(width: 900, height: 300), documentHeight: 80)
    #expect(textView.frame.width >= 900)
  }
#endif
