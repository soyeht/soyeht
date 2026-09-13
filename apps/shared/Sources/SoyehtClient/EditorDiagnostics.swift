import Foundation
import SoyehtProtocol

public enum EditorDiagnostics {
  public static func localRange(
    _ diagnostic: FileDiagnostic, in viewport: FileReadResult
  ) -> NSRange? {
    guard let first = viewport.lines.first?.line, let last = viewport.lines.last?.line,
      diagnostic.range.end.line >= first, diagnostic.range.start.line <= last
    else { return nil }
    let starts = lineStarts(viewport.lines)
    guard
      let start = localOffset(
        diagnostic.range.start, lines: viewport.lines, starts: starts, lower: first),
      let end = localOffset(
        diagnostic.range.end, lines: viewport.lines, starts: starts, lower: first)
    else { return nil }
    let upper = EditorEditing.viewportText(viewport.lines).utf16.count
    let boundedStart = min(start, upper)
    let boundedEnd = min(max(end, boundedStart), upper)
    let length =
      boundedEnd > boundedStart ? boundedEnd - boundedStart : min(1, upper - boundedStart)
    return length > 0 ? NSRange(location: boundedStart, length: length) : nil
  }

  private static func lineStarts(_ lines: [EditorLine]) -> [UInt32: Int] {
    var result: [UInt32: Int] = [:]
    var offset = 0
    for line in lines {
      result[line.line] = offset
      offset += line.text.utf16.count + 1
    }
    return result
  }

  private static func localOffset(
    _ position: TextPosition, lines: [EditorLine], starts: [UInt32: Int], lower: UInt32
  ) -> Int? {
    if position.line < lower { return 0 }
    guard let upper = lines.last?.line else { return nil }
    if position.line > upper { return EditorEditing.viewportText(lines).utf16.count }
    guard let offset = starts[position.line],
      let line = lines.first(where: { $0.line == position.line }),
      let column = Int(exactly: position.column), column <= line.text.utf16.count
    else { return nil }
    return offset + column
  }
}
