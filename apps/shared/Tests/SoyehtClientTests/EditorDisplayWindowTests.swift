import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func page(from: UInt32, count: Int, total: UInt32 = 100_000) -> FileReadResult {
  let file = FileDocument(
    fileId: "file-1", paneId: "pane-1", workspaceId: "workspace-1", path: "big.swift",
    language: .swift, revision: "1", diskRevision: "1", lineCount: total, byteCount: "1",
    dirty: false, conflicted: false, watching: false)
  let lines = (0..<count).map {
    EditorLine(line: from + UInt32($0), text: "// line \(from + UInt32($0))", highlights: [])
  }
  return FileReadResult(
    file: file, source: .buffer, fromLine: from, toLine: from + UInt32(count), lines: lines)
}

@Test func aPageSwappedInForwardIsHeldFromJustAboveTheView() {
  // The page that follows a scroll to line 1800 starts a quarter page above it.
  let from: UInt32 = 1_300
  let top = EditorDisplayWindow.top(
    pageFrom: from, pageTo: from + 2_000, current: 0, visibleFrom: 1_800)
  #expect(top == 1_800 - EditorDisplayWindow.lead)
  #expect(top > from)
}

@Test func theWindowNeverStartsAboveItsPageOrPastItsLastLine() {
  #expect(
    EditorDisplayWindow.top(pageFrom: 500, pageTo: 2_500, current: nil, visibleFrom: 520) == 500)
  #expect(
    EditorDisplayWindow.top(pageFrom: 0, pageTo: 2_000, current: nil, visibleFrom: 0) == 0)
  // A view below the page, while the page that holds it loads, keeps the last line.
  #expect(
    EditorDisplayWindow.top(pageFrom: 0, pageTo: 2_000, current: nil, visibleFrom: 9_000)
      == 1_999)
  #expect(EditorDisplayWindow.top(pageFrom: 0, pageTo: 0, current: nil, visibleFrom: 0) == 0)
}

@Test func theWindowStaysWhereItIsWhileTheViewMovesDownInsideThePage() {
  let top = EditorDisplayWindow.top(
    pageFrom: 1_300, pageTo: 3_300, current: 1_640, visibleFrom: 1_800)
  #expect(top == 1_640)
  #expect(
    EditorDisplayWindow.top(pageFrom: 1_300, pageTo: 3_300, current: 1_640, visibleFrom: 3_000)
      == 1_640)
}

@Test func theWindowMovesUpWhenTheViewComesBackToItsTop() {
  let near = 1_640 + EditorDisplayWindow.edge - 1
  let top = EditorDisplayWindow.top(
    pageFrom: 1_300, pageTo: 3_300, current: 1_640, visibleFrom: UInt32(near))
  #expect(top == near - EditorDisplayWindow.lead)
  #expect(top < 1_640)
  // A view still outside the edge leaves it.
  #expect(
    EditorDisplayWindow.top(
      pageFrom: 1_300, pageTo: 3_300, current: 1_640,
      visibleFrom: UInt32(1_640 + EditorDisplayWindow.edge)) == 1_640)
}

@Test func aLineThatMustBeHeldPullsTheWindowUpToIt() {
  #expect(
    EditorDisplayWindow.top(
      pageFrom: 1_300, pageTo: 3_300, current: 1_640, visibleFrom: 1_800, required: 1_500)
      == 1_500)
  #expect(
    EditorDisplayWindow.top(
      pageFrom: 1_300, pageTo: 3_300, current: 1_640, visibleFrom: 1_800, required: 1_900)
      == 1_640)
  // A line off the page is not the window's to hold.
  #expect(
    EditorDisplayWindow.top(
      pageFrom: 1_300, pageTo: 3_300, current: 1_640, visibleFrom: 1_800, required: 100)
      == 1_640)
}

@Test func aCaretAboveTheNewWindowTopIsHeldByTheWindowAndCarriedOver() {
  // The caret sits on line 1,550, above the 1,640 a window would be placed at.
  let old = page(from: 1_300, count: 2_000)
  let oldWindow = EditorDisplayWindow.viewport(old, top: 1_400)
  let oldText = EditorEditing.viewportText(oldWindow.lines)
  let caret = EditorViewportPaging.localOffset(TextPosition(line: 1_550, column: 4), in: oldWindow)
  let swapped = page(from: 1_500, count: 2_000)
  let top = EditorDisplayWindow.top(
    pageFrom: swapped.fromLine, pageTo: swapped.toLine, current: 1_400, visibleFrom: 1_800,
    required: 1_550)
  #expect(top == 1_550)
  let window = EditorDisplayWindow.viewport(swapped, top: top)
  let carried = EditorDisplayWindow.carried(
    [NSRange(location: caret ?? 0, length: 0)], text: oldText, from: 1_400, into: window)
  let wanted = EditorViewportPaging.localOffset(TextPosition(line: 1_550, column: 4), in: window)
  #expect(carried == [NSRange(location: wanted ?? -1, length: 0)])
  // Without the requirement the window would sit at 1,640 and the caret would be lost.
  let without = EditorDisplayWindow.viewport(swapped, top: 1_640)
  #expect(
    EditorDisplayWindow.carried(
      [NSRange(location: caret ?? 0, length: 0)], text: oldText, from: 1_400, into: without)
      == nil)
}

@Test func aRangeCarriedToTheMovedWindowKeepsItsLengthAndItsText() {
  let loaded = page(from: 1_300, count: 2_000)
  let before = EditorDisplayWindow.viewport(loaded, top: 1_640)
  let after = EditorDisplayWindow.viewport(loaded, top: 1_500)
  let text = EditorEditing.viewportText(before.lines)
  let start =
    EditorViewportPaging.localOffset(TextPosition(line: 1_700, column: 3), in: before) ?? 0
  let end = EditorViewportPaging.localOffset(TextPosition(line: 1_702, column: 5), in: before) ?? 0
  let moved = EditorDisplayWindow.carried(
    [NSRange(location: start, length: end - start)], text: text, from: 1_640, into: after)
  let range = moved?.first ?? NSRange(location: 0, length: 0)
  let selected = (EditorEditing.viewportText(after.lines) as NSString).substring(with: range)
  let original = (text as NSString).substring(with: NSRange(location: start, length: end - start))
  #expect(selected == original)
}

@Test func theWindowChangesOnlyAsTheTextOnScreenAllows() {
  // Select All and a go-to above the window both ask for a higher top.
  #expect(
    EditorDisplayWindow.change(desired: 1_300, current: 1_640, holdsPage: true, composing: false)
      == .extendUp(to: 1_300))
  // Lines the person typed are below the window's top, so the text is extended all the same.
  #expect(
    EditorDisplayWindow.change(desired: 1_500, current: 1_640, holdsPage: true, composing: false)
      == .extendUp(to: 1_500))
  // Composition keeps the text for now; the same request is granted when it ends.
  #expect(
    EditorDisplayWindow.change(desired: 1_500, current: 1_640, holdsPage: true, composing: true)
      == .keep)
  // A page that is not the one on screen is rendered again.
  #expect(
    EditorDisplayWindow.change(desired: 1_500, current: 1_640, holdsPage: false, composing: false)
      == .replace(top: 1_500))
  #expect(
    EditorDisplayWindow.change(desired: 4_000, current: 1_640, holdsPage: true, composing: false)
      == .replace(top: 4_000))
  #expect(
    EditorDisplayWindow.change(desired: 1_640, current: 1_640, holdsPage: true, composing: false)
      == .keep)
  #expect(
    EditorDisplayWindow.change(desired: 0, current: nil, holdsPage: true, composing: false)
      == .replace(top: 0))
}

@Test func aTargetAboveTheWindowIsLandedOnceTheWindowCoversIt() {
  let loaded = page(from: 1_300, count: 2_000)
  let window = EditorDisplayWindow.viewport(loaded, top: 1_640)
  let target = EditorNavigationTarget(
    id: "undo-1", fileId: "file-1", position: TextPosition(line: 1_500, column: 2),
    selectionEnd: nil)
  #expect(EditorViewportPaging.landing(target, in: window) == nil)
  func wanted(composing: Bool) -> EditorDisplayWindow.Change {
    let top = EditorDisplayWindow.top(
      pageFrom: loaded.fromLine, pageTo: loaded.toLine, current: 1_640, visibleFrom: 1_800,
      required: max(loaded.fromLine, target.position.line - EditorDisplayWindow.lead))
    return EditorDisplayWindow.change(
      desired: top, current: 1_640, holdsPage: true, composing: composing)
  }
  // While text is composed nothing is dropped: the target is still unapplied, the request stands.
  #expect(wanted(composing: true) == .keep)
  guard case .extendUp(let top) = wanted(composing: false) else {
    Issue.record("the window did not move up")
    return
  }
  #expect(
    EditorViewportPaging.landing(target, in: EditorDisplayWindow.viewport(loaded, top: top)) != nil)
}

@Test func aWindowOutsideTheNewPageIsPlacedAgain() {
  // The page moved forward and no longer holds the old top.
  #expect(
    EditorDisplayWindow.top(pageFrom: 4_000, pageTo: 6_000, current: 1_640, visibleFrom: 4_500)
      == 4_500 - EditorDisplayWindow.lead)
}

@Test func theWindowedPageKeepsLineNumbersAndTextOfWhatItHolds() {
  let loaded = page(from: 1_300, count: 2_000)
  let windowed = EditorDisplayWindow.viewport(loaded, top: 1_640)
  #expect(windowed.fromLine == 1_640)
  #expect(windowed.toLine == loaded.toLine)
  #expect(windowed.lines.count == 2_000 - 340)
  #expect(windowed.lines.first?.line == 1_640)
  #expect(windowed.lines.first?.text == "// line 1640")
  #expect(windowed.lines.last?.line == loaded.lines.last?.line)
  // Positions of held lines map to the same text as in the whole page.
  let position = TextPosition(line: 2_000, column: 3)
  let whole = EditorViewportPaging.localOffset(position, in: loaded)
  let held = EditorViewportPaging.localOffset(position, in: windowed)
  let wholeText = EditorEditing.viewportText(loaded.lines) as NSString
  let heldText = EditorEditing.viewportText(windowed.lines) as NSString
  #expect(wholeText.substring(from: whole ?? 0).hasPrefix(heldText.substring(from: held ?? 0)))
  // Lines above the window are not held.
  #expect(
    EditorViewportPaging.localOffset(TextPosition(line: 1_500, column: 0), in: windowed) == nil)
}

@Test func aTopAtOrAboveThePageStartIsThePageItself() {
  let loaded = page(from: 1_300, count: 100)
  #expect(EditorDisplayWindow.viewport(loaded, top: 1_300).lines.count == 100)
  #expect(EditorDisplayWindow.viewport(loaded, top: 0).fromLine == 1_300)
  #expect(EditorDisplayWindow.viewport(loaded, top: 9_999).lines.isEmpty)
}

#if os(macOS)
  import AppKit

  /// What a page swap makes the text view lay out before the first visible line can be
  /// drawn. It is counted in laid-out characters, not measured in time.
  @MainActor private struct SwappedPage {
    let scroll = NSScrollView(frame: NSRect(x: 0, y: 0, width: 600, height: 720))
    let textView = NSTextView(frame: NSRect(x: 0, y: 0, width: 600, height: 720))
    let lineHeight: CGFloat = 18
    let inset: CGFloat = 10
    let font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
    let shown: FileReadResult

    init(page: FileReadResult, top: UInt32, firstVisibleLine: UInt32) {
      shown = EditorDisplayWindow.viewport(page, top: top)
      scroll.documentView = textView
      let characterWidth = ("0" as NSString).size(withAttributes: [.font: font]).width
      EditorTextLayout.configure(textView)
      let paragraph = EditorTextLayout.paragraphStyle(
        lineHeight: lineHeight, characterWidth: characterWidth, wraps: false)
      textView.textContainerInset = NSSize(
        width: inset, height: inset + CGFloat(shown.fromLine) * lineHeight)
      textView.textStorage?.setAttributedString(
        NSAttributedString(
          string: EditorEditing.viewportText(shown.lines),
          attributes: [.font: font, .paragraphStyle: paragraph]))
      EditorTextLayout.fit(
        textView, visible: scroll.contentSize,
        documentHeight: CGFloat(page.file.lineCount) * lineHeight)
      scroll.contentView.scroll(
        to: NSPoint(x: 0, y: inset + CGFloat(firstVisibleLine) * lineHeight))
      scroll.reflectScrolledClipView(scroll.contentView)
    }

    /// Lays out what drawing the visible rect needs, as the text view does.
    func layOutVisible() {
      let visible = textView.visibleRect
      let origin = textView.textContainerOrigin
      let wanted = NSRect(
        x: visible.minX - origin.x, y: visible.minY - origin.y, width: visible.width,
        height: visible.height)
      textView.layoutManager?.ensureLayout(forBoundingRect: wanted, in: textView.textContainer!)
    }

    /// Lines laid out from the first line of the text.
    var laidOutLines: Int {
      let laid = textView.layoutManager?.firstUnlaidCharacterIndex() ?? 0
      let text = textView.string as NSString
      return text.substring(to: min(laid, text.length)).split(
        separator: "\n", omittingEmptySubsequences: false
      ).count - 1
    }

    /// Where the text view puts the first character of `line`, in the document.
    func documentY(ofLine line: UInt32) -> CGFloat? {
      guard line >= shown.fromLine, let layout = textView.layoutManager else { return nil }
      let text = textView.string as NSString
      var offset = 0
      for held in shown.lines.prefix(Int(line - shown.fromLine)) {
        offset += held.text.utf16.count + 1
      }
      guard offset < text.length else { return nil }
      let glyph = layout.glyphIndexForCharacter(at: offset)
      return layout.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil).minY
        + textView.textContainerOrigin.y
    }
  }

  @MainActor @Test func aSwappedPageLaysOutOnlyTheLinesFromTheWindowTopToTheView() {
    let loaded = page(from: 1_300, count: 2_000)
    let visibleFrom: UInt32 = 1_800
    let top = EditorDisplayWindow.top(
      pageFrom: loaded.fromLine, pageTo: loaded.toLine, current: nil, visibleFrom: visibleFrom)
    let windowed = SwappedPage(page: loaded, top: top, firstVisibleLine: visibleFrom)
    windowed.layOutVisible()
    let whole = SwappedPage(page: loaded, top: loaded.fromLine, firstVisibleLine: visibleFrom)
    whole.layOutVisible()

    let aboveView = Int(visibleFrom - top)
    let visibleLines = Int(ceil(720 / windowed.lineHeight))
    #expect(windowed.laidOutLines >= aboveView + visibleLines)
    #expect(windowed.laidOutLines <= aboveView + visibleLines + 2)
    // The whole page would lay out the quarter page above the view as well.
    #expect(whole.laidOutLines >= Int(visibleFrom - loaded.fromLine) + visibleLines)
    #expect(windowed.laidOutLines * 2 < whole.laidOutLines)
  }

  @MainActor @Test func aWindowedPageDrawsEachLineWhereTheWholePageDoes() throws {
    let loaded = page(from: 1_300, count: 2_000)
    let visibleFrom: UInt32 = 1_800
    let windowed = SwappedPage(page: loaded, top: 1_640, firstVisibleLine: visibleFrom)
    windowed.layOutVisible()
    let whole = SwappedPage(page: loaded, top: loaded.fromLine, firstVisibleLine: visibleFrom)
    whole.layOutVisible()
    for line in [visibleFrom, visibleFrom + 20, visibleFrom + 39] {
      let held = try #require(windowed.documentY(ofLine: line))
      let all = try #require(whole.documentY(ofLine: line))
      #expect(held == all)
      #expect(held == windowed.inset + CGFloat(line) * windowed.lineHeight)
    }
  }

  @MainActor @Test func extendingTheWindowKeepsSelectionsAndLocalEditsAndCoversThePage() throws {
    let loaded = page(from: 1_300, count: 2_000)
    let swapped = SwappedPage(page: loaded, top: 1_640, firstVisibleLine: 1_800)
    let textView = swapped.textView
    // A selection and a caret in the window, and an edit the engine does not know yet.
    let storage = try #require(textView.textStorage)
    let edit = NSRange(location: 40, length: 0)
    storage.replaceCharacters(in: edit, with: "TYPED")
    let selections = [NSRange(location: 100, length: 12), NSRange(location: 300, length: 5)]
    textView.selectedRanges = selections.map(NSValue.init(range:))
    let before = textView.string

    let head = EditorDisplayWindow.lines(loaded, from: loaded.fromLine, to: 1_640)
    let text = EditorEditing.viewportText(head.lines) + "\n"
    EditorTextLayout.prepend(NSAttributedString(string: text), to: textView)

    #expect(head.lines.count == 340)
    #expect(textView.string == text + before)
    #expect(textView.string.contains("TYPED"))
    let shift = (text as NSString).length
    #expect(
      textView.selectedRanges.map(\.rangeValue)
        == selections.map { NSRange(location: $0.location + shift, length: $0.length) })
    // Select All now covers the whole loaded page, with the local edit where it was typed.
    var whole = EditorEditing.viewportText(loaded.lines) as NSString
    whole =
      whole.replacingCharacters(
        in: NSRange(location: shift + 40, length: 0), with: "TYPED") as NSString
    #expect(textView.string == whole as String)
  }
#endif
