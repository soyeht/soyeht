import Foundation
import SoyehtProtocol

/// The part of the loaded page the text view holds.
///
/// A text view lays its text out from the first line down to the last line on screen. A page
/// has a quarter of its lines above the view (`EditorViewportPaging`), so the swap of a page
/// that moved forward would lay those lines out at once, in the frame that shows the new page,
/// before the first visible line can be placed. The window drops them: the text view holds the
/// page from `top`, a few lines above the view, and the lines above it stay in the page until
/// the view comes back up to them. Everything that maps text to lines uses the windowed page
/// (`viewport(_:top:)`); paging still reads the loaded page.
public enum EditorDisplayWindow {
  /// Lines kept above the first visible line when the window is placed.
  public static let lead = EditorViewportPaging.prefetchLines
  /// How close to the window's top the view may come before the window moves up.
  public static let edge = lead / 4

  /// The first line the text view holds for the page `pageFrom..<pageTo`.
  ///
  /// `current` is the top of the window now, `visibleFrom` the first line on screen and
  /// `required` the first line that has to be held: a caret or selection, or a navigation
  /// target. A line outside the page is not required. The window keeps its top while the view
  /// stays below it, and moves it up when the view came within `edge` lines of it or when
  /// `required` lies above it. It is placed anew when the page moved away from it. A top above
  /// the page's first line is the page itself.
  public static func top(
    pageFrom: UInt32, pageTo: UInt32, current: UInt32?, visibleFrom: UInt32,
    required: UInt32? = nil
  ) -> UInt32 {
    let last = max(pageFrom, pageTo == 0 ? 0 : pageTo - 1)
    let placed = min(max(pageFrom, visibleFrom >= lead ? visibleFrom - lead : 0), last)
    var base = placed
    if let current, current >= pageFrom, current <= last {
      base = current > pageFrom && visibleFrom < current + edge ? placed : current
    }
    if let required, required >= pageFrom, required < pageTo { base = min(base, required) }
    return max(pageFrom, base)
  }

  /// What the text view does to reach the window `desired`.
  public enum Change: Equatable, Sendable {
    /// The text on screen already holds it, or must not be touched yet.
    case keep
    /// The lines from the new top up to the current one are put before the text.
    case extendUp(to: UInt32)
    /// The text is rendered again from the new top.
    case replace(top: UInt32)
  }

  /// `holdsPage` is true when the text on screen came from this very page, down to its last
  /// line, so that only lines above it are missing. Local edits do not change that: lines above
  /// the window cannot be edited. Text being composed stays as it is, and the change is made
  /// once the composition is over.
  public static func change(
    desired: UInt32, current: UInt32?, holdsPage: Bool, composing: Bool
  ) -> Change {
    guard let current, holdsPage else { return .replace(top: desired) }
    if desired == current { return .keep }
    if desired < current { return composing ? .keep : .extendUp(to: desired) }
    return .replace(top: desired)
  }

  /// The lines of `page` from `from` up to, not including, `to`.
  public static func lines(_ page: FileReadResult, from: UInt32, to: UInt32) -> FileReadResult {
    var head = viewport(page, top: from)
    let count = min(Int(to > head.fromLine ? to - head.fromLine : 0), head.lines.count)
    head.lines = Array(head.lines.prefix(count))
    head.toLine = head.fromLine + UInt32(count)
    return head
  }

  /// `ranges` of the text on screen, which starts at line `previousStart`, moved to where the
  /// same lines sit in `viewport`. Nil when an end of one of them is not held by `viewport`.
  public static func carried(
    _ ranges: [NSRange], text: String, from previousStart: UInt32, into viewport: FileReadResult
  ) -> [NSRange]? {
    var moved: [NSRange] = []
    for range in ranges {
      guard
        let start = EditorEditing.textPosition(
          text: text, viewportStartLine: previousStart, utf16Offset: range.location),
        let end = EditorEditing.textPosition(
          text: text, viewportStartLine: previousStart, utf16Offset: NSMaxRange(range)),
        let from = EditorViewportPaging.localOffset(start, in: viewport),
        let to = EditorViewportPaging.localOffset(end, in: viewport)
      else { return nil }
      moved.append(NSRange(location: from, length: max(0, to - from)))
    }
    return moved
  }

  /// `page` without the lines before `top`; the page itself when `top` is not past its start.
  public static func viewport(_ page: FileReadResult, top: UInt32) -> FileReadResult {
    guard top > page.fromLine else { return page }
    let dropped = min(Int(top - page.fromLine), page.lines.count)
    var windowed = page
    windowed.fromLine = page.fromLine + UInt32(dropped)
    windowed.lines = Array(page.lines.dropFirst(dropped))
    return windowed
  }
}
