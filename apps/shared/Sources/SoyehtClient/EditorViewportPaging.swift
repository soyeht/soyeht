import Foundation
import SoyehtProtocol

public enum EditorViewportPaging {
  public static let pageLines = UInt32(ProtocolLimits.editorViewportLines)
  public static let prefetchLines = UInt32(ClientPolicy.editorPrefetchLines)

  /// The first line of the page to read, or nil while the loaded page holds the view with room
  /// to spare, or when no other page would hold more of it. `askedLines` is how many lines were
  /// asked for when the loaded page was read; nil means as many as it holds.
  ///
  /// A page the engine cut short (it stops a page at its byte budget) is as long as a page can be
  /// there: it is settled when it holds the view, and one that does not is followed by the page
  /// that starts at the view, after which nothing more is asked.
  public static func requestedStart(
    visibleFrom: UInt32, visibleLines: UInt32, loadedFrom: UInt32, loadedTo: UInt32,
    totalLines: UInt32, askedLines: UInt32? = nil
  ) -> UInt32? {
    let loadedCount = loadedTo.saturatingSubtract(loadedFrom)
    guard totalLines > loadedCount else { return nil }
    let visibleTo = Swift.min(visibleFrom.saturatingAdd(max(visibleLines, 1)), totalLines)
    let cut = loadedCount < (askedLines ?? loadedCount)
    let margin = min(prefetchLines, loadedCount / 4)
    let protectedFrom = loadedFrom.saturatingAdd(margin)
    let protectedTo = loadedTo.saturatingSubtract(margin)
    if loadedCount >= pageLines || cut, visibleFrom >= protectedFrom, visibleTo <= protectedTo {
      return nil
    }
    let lead = pageLines / UInt32(ClientPolicy.editorPageLeadDivisor)
    let lastStart = totalLines.saturatingSubtract(pageLines)
    let start = Swift.min(visibleFrom.saturatingSubtract(lead), lastStart)
    if cut {
      if visibleFrom >= loadedFrom, visibleTo <= loadedTo { return nil }
      let beginsInside = visibleFrom >= loadedFrom && visibleFrom < loadedTo
      if !beginsInside, start != loadedFrom { return start }
      return visibleFrom == loadedFrom ? nil : visibleFrom
    }
    if start != loadedFrom { return start }
    // The page that leads the view is the one loaded. A full page is the most there is, and a
    // short one that was asked for as short is read in full, unless it already ends the file.
    return loadedCount < pageLines && loadedTo < totalLines ? start : nil
  }

  /// The first line to load so `line` is visible: the loaded page when it
  /// already holds the line, otherwise a page with the line a quarter down.
  /// `askedLines` is how many lines were asked for when the loaded page was read; a page that
  /// came back shorter was cut short by the engine, the next may be as short, and the line then
  /// leads by a quarter of that page, so that the page read holds it. A page that has to be read
  /// again for it starts at the line itself.
  public static func landingStart(
    line: UInt32, loadedFrom: UInt32, loadedTo: UInt32, askedLines: UInt32? = nil
  ) -> UInt32 {
    if line >= loadedFrom, line < loadedTo { return loadedFrom }
    let loaded = loadedTo.saturatingSubtract(loadedFrom)
    let lead = pageLines / UInt32(ClientPolicy.editorPageLeadDivisor)
    let cut = loaded < (askedLines ?? loaded)
    return line.saturatingSubtract(cut ? Swift.min(lead, loaded / 4) : lead)
  }

  /// The UTF-16 offset of `position` in the loaded page text, clamped to the
  /// document: a column past its line's end lands at that end, and a line
  /// past the last one lands at the end of the document. Nil when the
  /// position lies on a line outside the loaded page.
  public static func localOffset(_ position: TextPosition, in viewport: FileReadResult) -> Int? {
    var line = position.line
    var column = Int(position.column)
    if line >= viewport.file.lineCount, viewport.toLine >= viewport.file.lineCount,
      let last = viewport.lines.last
    {
      line = last.line
      column = Int.max
    }
    guard line >= viewport.fromLine, line < viewport.toLine else { return nil }
    let index = Int(line - viewport.fromLine)
    guard index < viewport.lines.count else { return nil }
    let prefix = viewport.lines.prefix(index).reduce(0) { $0 + $1.text.utf16.count + 1 }
    return prefix + min(column, viewport.lines[index].text.utf16.count)
  }

  /// Where a navigation lands in the loaded page: a caret at `position`, or
  /// the selection up to `selectionEnd`, both clamped like `localOffset`.
  public static func landing(_ target: EditorNavigationTarget, in viewport: FileReadResult)
    -> NSRange?
  {
    guard let start = localOffset(target.position, in: viewport) else { return nil }
    let end = target.selectionEnd.flatMap { localOffset($0, in: viewport) } ?? start
    return NSRange(location: start, length: max(0, end - start))
  }
}

extension UInt32 {
  fileprivate func saturatingAdd(_ other: UInt32) -> UInt32 {
    let result = addingReportingOverflow(other)
    return result.overflow ? .max : result.partialValue
  }

  fileprivate func saturatingSubtract(_ other: UInt32) -> UInt32 {
    self >= other ? self - other : 0
  }
}
