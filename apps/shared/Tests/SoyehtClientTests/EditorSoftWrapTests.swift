import Testing

@testable import SoyehtClient

@Test func withoutWrapEveryLineIsOneRowAndOffsetsMapToLines() {
  let layout = EditorWrapLayout(text: "one\n\nthree long line", columns: nil)
  #expect(layout.lineCount == 3)
  #expect(layout.rowCount == 3)
  #expect(layout.lineStarts == [0, 4, 5, 21])
  #expect(layout.line(containing: 0) == 0)
  #expect(layout.line(containing: 3) == 0)
  #expect(layout.line(containing: 4) == 1)
  #expect(layout.line(containing: 5) == 2)
  #expect(layout.line(containing: 99) == 2)
}

@Test func longLinesTakeOneRowPerColumnsCellsAndLineNumbersStayOnLogicalLines() {
  let long = String(repeating: "x", count: 25)
  let layout = EditorWrapLayout(text: "short\n\(long)\n\nexact10chr", columns: 10)
  #expect(layout.lineCount == 4)
  #expect((0..<4).map(layout.rows(ofLine:)) == [1, 3, 1, 1])
  #expect(layout.rowStarts == [0, 1, 4, 5, 6])
  // Rows 1, 2 and 3 all belong to the long line; the gutter labels only row 1.
  #expect((0..<6).map(layout.line(atRow:)) == [0, 1, 1, 1, 2, 3])
  #expect(layout.line(atRow: 99) == 3)
}

@Test func tabsRunToTheNextStopAndMarksTakeNoCell() {
  // "ab\t" ends at cell 4; "cdefgh" fills cells 4-9; "i" wraps.
  #expect(EditorWrapLayout(text: "ab\tcdefghi", columns: 10).rowCount == 2)
  #expect(EditorWrapLayout(text: "ab\tcdefgh", columns: 10).rowCount == 1)
  // Ten letters with combining acute accents still fit ten cells.
  let accented = String(repeating: "e\u{301}", count: 10)
  #expect(EditorWrapLayout(text: accented, columns: 10).rowCount == 1)
  // A surrogate pair is one cell but two UTF-16 units.
  let emoji = EditorWrapLayout(text: "😀😀\nx", columns: 10)
  #expect(emoji.lineStarts == [0, 5, 7])
}

@Test func unloadedLinesCountOneRowSoPagingMapsRowsBackToLogicalLines() {
  let page = EditorWrapLayout(text: "a\n\(String(repeating: "b", count: 30))\nc", columns: 10)
  // Lines 100-102 are loaded; line 101 takes three rows.
  #expect(page.documentRows(totalLines: 500) == 502)
  #expect(page.logicalLine(atDocumentRow: 50, fromLine: 100) == 50)
  #expect(page.logicalLine(atDocumentRow: 100, fromLine: 100) == 100)
  #expect(page.logicalLine(atDocumentRow: 103, fromLine: 100) == 101)
  #expect(page.logicalLine(atDocumentRow: 104, fromLine: 100) == 102)
  #expect(page.logicalLine(atDocumentRow: 105, fromLine: 100) == 103)
  #expect(page.logicalLine(atDocumentRow: -4, fromLine: 100) == 0)
}

@Test func spacesHangPastTheRowEndAndTheNextCharacterStartsTheRow() {
  // Eight cells: "abcdef" then three space characters hang on row one, "g" opens row two.
  #expect(EditorWrapLayout(text: "abcdef   g", columns: 8).rowCount == 2)
  #expect(
    EditorWrapLayout(text: "abcdef" + String(repeating: " ", count: 30), columns: 8).rowCount == 1)
  // A no-break space does not hang.
  #expect(EditorWrapLayout(text: "abcdefgh\u{a0}", columns: 8).rowCount == 2)
}

@Test func columnsFitTheWidthAndNeverDropBelowOne() {
  #expect(EditorWrapLayout.columns(fitting: 72.9, characterWidth: 7.2) == 10)
  #expect(EditorWrapLayout.columns(fitting: 3, characterWidth: 7.2) == 1)
  #expect(EditorWrapLayout.columns(fitting: 100, characterWidth: 0) == 1)
}

#if os(macOS)
  import AppKit

  @MainActor private struct WrappedTextView {
    let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 300, height: 400))
    let textView = NSTextView(frame: NSRect(x: 0, y: 0, width: 300, height: 400))
    let font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
    let columns: Int

    init(_ text: String) {
      scroll.documentView = textView
      textView.textContainerInset = NSSize(width: 10, height: 10)
      let characterWidth = ("0" as NSString).size(withAttributes: [.font: font]).width
      columns = EditorTextLayout.wrapColumns(
        textView, visibleWidth: scroll.contentSize.width, characterWidth: characterWidth)
      EditorTextLayout.configure(
        textView, wrapColumns: columns, characterWidth: characterWidth)
      let paragraph = EditorTextLayout.paragraphStyle(
        lineHeight: 19, characterWidth: characterWidth, wraps: true)
      textView.textStorage?.setAttributedString(
        NSAttributedString(string: text, attributes: [.font: font, .paragraphStyle: paragraph]))
      EditorTextLayout.fit(textView, visible: scroll.contentSize, documentHeight: 400)
      textView.layoutManager?.ensureLayout(for: textView.textContainer!)
    }

    /// Rows AppKit laid out for each line.
    func rows() -> [Int] {
      let layout = textView.layoutManager!
      let text = textView.string as NSString
      var rows: [Int] = []
      text.enumerateSubstrings(
        in: NSRange(location: 0, length: text.length), options: [.byParagraphs]
      ) { _, range, _, _ in
        var count = 0
        let glyphs = layout.glyphRange(forCharacterRange: range, actualCharacterRange: nil)
        var index = glyphs.location
        repeat {
          var fragment = NSRange()
          layout.lineFragmentRect(forGlyphAt: index, effectiveRange: &fragment)
          count += 1
          index = NSMaxRange(fragment)
        } while index < NSMaxRange(glyphs)
        rows.append(count)
      }
      return rows
    }

    /// Where a click at `point` of the text view puts the caret: the nearer insertion point.
    func offset(at point: NSPoint) -> Int {
      let local = NSPoint(
        x: point.x - textView.textContainerOrigin.x, y: point.y - textView.textContainerOrigin.y)
      var fraction: CGFloat = 0
      let index = textView.layoutManager!.characterIndex(
        for: local, in: textView.textContainer!,
        fractionOfDistanceBetweenInsertionPoints: &fraction)
      return fraction > 0.5 ? index + 1 : index
    }
  }

  /// AppKit breaks wrapped code where the shared layout says, so the gutter, the scroll extent
  /// and paging agree with what is drawn.
  @MainActor @Test func theMacTextViewWrapsWhereTheSharedLayoutDoes() {
    let lines = [
      "short",
      String(repeating: "abcdefghij", count: 9),
      "let value = compute(first, second) + another_long_identifier_name * 3 // comment",
      "\tif x {\t\treturn y }\t" + String(repeating: "z", count: 50),
      "",
      String(repeating: " ", count: 70) + "end",
      String(repeating: "a", count: 35) + "  bcd",
      String(repeating: "a", count: 34) + "\tb\t" + String(repeating: "c", count: 40),
      String(repeating: "ab ", count: 30),
      String(repeating: "a", count: 40) + "\u{a0}b",
    ]
    let text = lines.joined(separator: "\n")
    let view = WrappedTextView(text)
    #expect(view.columns > 20)
    let shared = EditorWrapLayout(text: text, columns: view.columns)
    #expect(view.rows() == (0..<shared.lineCount).map(shared.rows), "columns \(view.columns)")
    #expect(!view.scroll.hasHorizontalScroller || view.textView.frame.width <= 300)
  }

  /// The caret moves down through the rows of one wrapped line before the next line, and a
  /// click right of a short wrapped row lands at that row's end.
  @MainActor @Test func theCaretMovesAcrossWrappedRowsAndClicksPastARowEndLandAtIt() {
    let long = String(repeating: "abcdefghij", count: 9)
    let view = WrappedTextView(long + "\nnext")
    let columns = view.columns
    view.textView.setSelectedRange(NSRange(location: 2, length: 0))
    view.textView.moveDown(nil)
    #expect(view.textView.selectedRange().location == columns + 2)
    view.textView.moveDown(nil)
    #expect(view.textView.selectedRange().location == 2 * columns + 2)

    let origin = view.textView.textContainerOrigin
    // Right of the "next" line, which is short.
    let rows = EditorWrapLayout(text: view.textView.string, columns: columns).rowCount
    let nextRow = CGFloat(rows - 1) * 19 + 9 + origin.y
    let length = (view.textView.string as NSString).length
    #expect(view.offset(at: NSPoint(x: 280, y: nextRow)) == length)
    #expect(view.offset(at: NSPoint(x: origin.x + 1, y: nextRow)) == length - 4)
    // Right of the first row of the wrapped line: the end of that row.
    #expect(view.offset(at: NSPoint(x: 295, y: origin.y + 9)) == columns)
  }
#endif
