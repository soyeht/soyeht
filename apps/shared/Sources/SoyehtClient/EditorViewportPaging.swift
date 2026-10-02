import Foundation
import SoyehtProtocol

public enum EditorViewportPaging {
  public static let pageLines = UInt32(ProtocolLimits.editorViewportLines)
  public static let prefetchLines: UInt32 = 160

  public static func requestedStart(
    visibleFrom: UInt32, visibleLines: UInt32, loadedFrom: UInt32, loadedTo: UInt32,
    totalLines: UInt32
  ) -> UInt32? {
    guard totalLines > loadedTo - loadedFrom else { return nil }
    let visibleTo = Swift.min(visibleFrom.saturatingAdd(max(visibleLines, 1)), totalLines)
    let loadedCount = loadedTo.saturatingSubtract(loadedFrom)
    let margin = min(prefetchLines, loadedCount / 4)
    let protectedFrom = loadedFrom.saturatingAdd(margin)
    let protectedTo = loadedTo.saturatingSubtract(margin)
    if loadedCount >= pageLines, visibleFrom >= protectedFrom, visibleTo <= protectedTo {
      return nil
    }
    let lead = pageLines / 4
    let lastStart = totalLines.saturatingSubtract(pageLines)
    let start = Swift.min(visibleFrom.saturatingSubtract(lead), lastStart)
    if start == loadedFrom, loadedCount >= pageLines { return nil }
    return start
  }

  /// The first line to load so `line` is visible: the loaded page when it
  /// already holds the line, otherwise a page with the line a quarter down.
  public static func landingStart(line: UInt32, loadedFrom: UInt32, loadedTo: UInt32) -> UInt32 {
    if line >= loadedFrom, line < loadedTo { return loadedFrom }
    return line.saturatingSubtract(pageLines / 4)
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
