import Foundation
import SoyehtProtocol

/// The text of the lines a writing viewport holds, and the offsets that name its lines. Every
/// writing surface edits one of these, so the Mac and iOS panes resolve a position, clamp a
/// selection and find a line the same way.
public struct WritingViewportText: Equatable, Sendable {
  public var source: String
  public var firstLine: UInt32

  public init(source: String = "", firstLine: UInt32 = 0) {
    self.source = source
    self.firstLine = firstLine
  }

  public init(lines: [EditorLine], firstLine: UInt32) {
    self.init(source: EditorEditing.viewportText(lines), firstLine: firstLine)
  }

  /// Replaces `range` of `source` with `replacement`. Returns the edit to send to the engine and
  /// the offset just after the replacement, or nil, leaving the text as it was, when `range`
  /// does not lie in it.
  public mutating func replace(
    _ range: NSRange, with replacement: String
  ) -> (edit: TextEdit, cursor: Int)? {
    let raw = source as NSString
    guard NSMaxRange(range) <= raw.length,
      let start = position(utf16Offset: range.location),
      let end = position(utf16Offset: NSMaxRange(range))
    else { return nil }
    source = raw.replacingCharacters(in: range, with: replacement)
    return (
      TextEdit(range: TextRange(start: start, end: end), text: replacement),
      range.location + replacement.utf16.count
    )
  }

  /// The document position of a UTF-16 offset into `source`.
  public func position(utf16Offset: Int) -> TextPosition? {
    EditorEditing.textPosition(text: source, viewportStartLine: firstLine, utf16Offset: utf16Offset)
  }

  /// The line holding a UTF-16 offset, or the first line when the offset is outside `source`.
  public func line(utf16Offset: Int) -> UInt32 {
    position(utf16Offset: utf16Offset)?.line ?? firstLine
  }

  /// `range` kept inside `source`; a range that starts outside it becomes the start.
  public func clamped(_ range: NSRange) -> NSRange {
    let length = source.utf16.count
    guard range.location != NSNotFound, range.location <= length else {
      return NSRange(location: 0, length: 0)
    }
    return NSRange(location: range.location, length: min(range.length, length - range.location))
  }

  /// The UTF-16 offset where a document line starts, if the viewport holds it.
  public func utf16Location(ofLine line: UInt32) -> Int? {
    guard line >= firstLine else { return nil }
    let relative = Int(line - firstLine)
    let lines = source.components(separatedBy: "\n")
    guard relative < lines.count else { return nil }
    return lines.prefix(relative).reduce(0) { $0 + $1.utf16.count + 1 }
  }

  /// The UTF-16 range of the document lines `start` through `end` (without the last break).
  public func range(fromLine start: UInt32, throughLine end: UInt32) -> NSRange? {
    guard start >= firstLine, end >= start else { return nil }
    let lines = source.components(separatedBy: "\n")
    let localStart = Int(start - firstLine)
    let localEnd = Int(end - firstLine)
    guard localStart < lines.count else { return nil }
    let clampedEnd = min(localEnd, lines.count - 1)
    let location = lines.prefix(localStart).reduce(0) { $0 + $1.utf16.count + 1 }
    let length = lines[localStart...clampedEnd].reduce(0) { $0 + $1.utf16.count + 1 } - 1
    return NSRange(location: location, length: max(0, length))
  }

  /// Each screenplay element of `structure` that the viewport holds, with its range in `source`.
  public func elementRanges(
    in structure: WritingStructureResult
  ) -> [(kind: ScreenplayElementKind, range: NSRange)] {
    structure.elements.compactMap { element in
      range(fromLine: element.startLine, throughLine: element.endLine).map { (element.kind, $0) }
    }
  }

  /// The range of every line of `structure` that has alternate takes and is in the viewport.
  public func variantRanges(in structure: WritingStructureResult) -> [NSRange] {
    structure.variants.compactMap { range(fromLine: $0.line, throughLine: $0.line) }
  }

  /// The UTF-16 range of every non-empty line except `activeLine`, for the focus mode that
  /// dims what the person is not writing.
  public func inactiveLineRanges(activeLine: UInt32) -> [NSRange] {
    guard activeLine >= firstLine else { return [] }
    let active = Int(activeLine - firstLine)
    var location = 0
    var ranges: [NSRange] = []
    for (index, line) in source.components(separatedBy: "\n").enumerated() {
      if index != active, !line.isEmpty {
        ranges.append(NSRange(location: location, length: line.utf16.count))
      }
      location += line.utf16.count + 1
    }
    return ranges
  }

  /// `lineRange` without its trailing line breaks.
  public func contentRange(ofLine lineRange: NSRange) -> NSRange {
    let raw = source as NSString
    var length = lineRange.length
    while length > 0 {
      let value = raw.character(at: lineRange.location + length - 1)
      if value != 10 && value != 13 { break }
      length -= 1
    }
    return NSRange(location: lineRange.location, length: length)
  }

  /// The UTF-16 range of the line holding `location`, with its breaks removed.
  public func contentRange(atLocation location: Int) -> NSRange {
    let raw = source as NSString
    let lineRange = raw.lineRange(for: NSRange(location: min(location, raw.length), length: 0))
    return contentRange(ofLine: lineRange)
  }
}
