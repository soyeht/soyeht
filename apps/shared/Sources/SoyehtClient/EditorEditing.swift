import Foundation
import SoyehtProtocol

public struct EditorLocalReplacement: Equatable {
  public let range: NSRange
  public let text: String

  public init(range: NSRange, text: String) {
    self.range = range
    self.text = text
  }
}

public struct EditorInputPlan {
  public let replacements: [EditorLocalReplacement]
  public let selections: [NSRange]
  public let edits: [TextEdit]

  public init(
    replacements: [EditorLocalReplacement], selections: [NSRange], edits: [TextEdit]
  ) {
    self.replacements = replacements
    self.selections = selections
    self.edits = edits
  }
}

public enum EditorEditing {
  public static func viewportText(_ lines: [EditorLine]) -> String {
    lines.map(\.text).joined(separator: "\n")
  }

  public static func insertionPlan(
    text: String, viewportStartLine: UInt32, ranges: [NSRange], inserted: String,
    language: EditorLanguage
  ) -> EditorInputPlan? {
    let source = text as NSString
    guard let ordered = validRanges(ranges, length: source.length) else { return nil }
    var changes: [(EditorLocalReplacement, Int)] = []
    for range in ordered {
      let selected = source.substring(with: range)
      if range.length == 0, let closing = closingCharacter(inserted),
        character(at: range.location, in: source) == closing
      {
        changes.append((EditorLocalReplacement(range: range, text: ""), 1))
      } else if let closing = closingCharacter(inserted) {
        let replacement = inserted + selected + closing
        changes.append(
          (
            EditorLocalReplacement(range: range, text: replacement),
            inserted.utf16.count + selected.utf16.count
          ))
      } else if inserted == "\n" {
        let indent = indentation(at: range.location, in: source)
        let extra =
          linePrefix(at: range.location, in: source).trimmingCharacters(in: .whitespaces)
            .last.map { "{[(".contains($0) } == true ? "    " : ""
        let replacement = "\n" + indent + extra
        changes.append(
          (EditorLocalReplacement(range: range, text: replacement), replacement.utf16.count))
      } else {
        changes.append((EditorLocalReplacement(range: range, text: inserted), inserted.utf16.count))
      }
    }
    return makePlan(
      source: source, viewportStartLine: viewportStartLine, changes: changes)
  }

  public static func deletionPlan(
    text: String, viewportStartLine: UInt32, ranges: [NSRange], backward: Bool,
    language: EditorLanguage
  ) -> EditorInputPlan? {
    let source = text as NSString
    guard let ordered = validRanges(ranges, length: source.length) else { return nil }
    let expanded = ordered.map { range -> NSRange in
      guard range.length == 0 else { return range }
      if backward, range.location > 0 {
        let previous = source.rangeOfComposedCharacterSequence(at: range.location - 1)
        if let closing = closingCharacter(source.substring(with: previous)),
          character(at: range.location, in: source) == closing
        {
          let next = source.rangeOfComposedCharacterSequence(at: range.location)
          return NSRange(location: previous.location, length: previous.length + next.length)
        }
        return previous
      }
      if !backward, range.location < source.length {
        return source.rangeOfComposedCharacterSequence(at: range.location)
      }
      return range
    }
    let changes = expanded.map { (EditorLocalReplacement(range: $0, text: ""), 0) }
    return makePlan(
      source: source, viewportStartLine: viewportStartLine, changes: changes)
  }

  public static func toggleCommentPlan(
    text: String, viewportStartLine: UInt32, ranges: [NSRange], language: EditorLanguage
  ) -> EditorInputPlan? {
    let source = text as NSString
    guard let ordered = validRanges(ranges, length: source.length), !ordered.isEmpty else {
      return nil
    }
    let starts = lineStarts(source)
    let indexes = Set(
      ordered.flatMap { selectedLineIndexes($0, starts: starts, length: source.length) }
    )
    .sorted()
    guard !indexes.isEmpty else { return nil }
    let marker = commentMarker(language)
    let details = indexes.map { lineDetail($0, starts: starts, source: source, marker: marker) }
    let removing = details.allSatisfy(\.commented)
    let replacements = details.map { detail -> EditorLocalReplacement in
      if removing {
        return EditorLocalReplacement(
          range: NSRange(location: detail.contentStart, length: detail.markerLength), text: "")
      }
      return EditorLocalReplacement(
        range: NSRange(location: detail.contentStart, length: 0), text: marker + " ")
    }
    let changes = replacements.map { ($0, 0) }
    return makePlan(
      source: source, viewportStartLine: viewportStartLine, changes: changes)
  }

  public static func nextOccurrence(text: String, ranges: [NSRange]) -> NSRange? {
    let source = text as NSString
    guard let selected = ranges.first(where: { $0.length > 0 && NSMaxRange($0) <= source.length })
    else { return nil }
    let needle = source.substring(with: selected)
    let after = NSRange(
      location: NSMaxRange(selected), length: source.length - NSMaxRange(selected))
    let found = source.range(of: needle, options: [], range: after)
    if found.location != NSNotFound, !ranges.contains(found) { return found }
    let before = NSRange(location: 0, length: selected.location)
    let wrapped = source.range(of: needle, options: [], range: before)
    return wrapped.location != NSNotFound && !ranges.contains(wrapped) ? wrapped : nil
  }

  public static func wordRange(text: String, at offset: Int) -> NSRange? {
    let source = text as NSString
    guard source.length > 0, offset >= 0, offset <= source.length else { return nil }
    var start = min(offset, source.length - 1)
    if !isWord(source.character(at: start)), start > 0 { start -= 1 }
    guard isWord(source.character(at: start)) else { return nil }
    var end = start + 1
    while start > 0, isWord(source.character(at: start - 1)) { start -= 1 }
    while end < source.length, isWord(source.character(at: end)) { end += 1 }
    return NSRange(location: start, length: end - start)
  }

  public static func applying(_ replacements: [EditorLocalReplacement], to text: String) -> String {
    let result = NSMutableString(string: text)
    for replacement in replacements.sorted(by: { $0.range.location > $1.range.location }) {
      result.replaceCharacters(in: replacement.range, with: replacement.text)
    }
    return result as String
  }

  public static func replacementPlan(
    text: String, viewportStartLine: UInt32, replacements: [EditorLocalReplacement],
    selections: [NSRange]
  ) -> EditorInputPlan? {
    let source = text as NSString
    guard validRanges(replacements.map(\.range), length: source.length) != nil else { return nil }
    let resultLength = replacements.reduce(source.length) {
      $0 - $1.range.length + $1.text.utf16.count
    }
    guard
      selections.allSatisfy({
        $0.location != NSNotFound && $0.location >= 0 && NSMaxRange($0) <= resultLength
      }),
      let edits = protocolEdits(
        source: source, viewportStartLine: viewportStartLine, replacements: replacements)
    else { return nil }
    return EditorInputPlan(replacements: replacements, selections: selections, edits: edits)
  }

  public static func textPosition(
    text: String, viewportStartLine: UInt32, utf16Offset: Int
  ) -> TextPosition? {
    position(source: text as NSString, offset: utf16Offset, base: viewportStartLine)
  }

  private static func makePlan(
    source: NSString, viewportStartLine: UInt32,
    changes: [(EditorLocalReplacement, Int)]
  ) -> EditorInputPlan? {
    let replacements = changes.map(\.0)
    guard
      let edits = protocolEdits(
        source: source, viewportStartLine: viewportStartLine, replacements: replacements)
    else { return nil }
    var delta = 0
    var selections: [NSRange] = []
    for (replacement, cursorOffset) in changes {
      let location = replacement.range.location + delta + cursorOffset
      selections.append(NSRange(location: location, length: 0))
      delta += replacement.text.utf16.count - replacement.range.length
    }
    return EditorInputPlan(replacements: replacements, selections: selections, edits: edits)
  }

  private static func protocolEdits(
    source: NSString, viewportStartLine: UInt32, replacements: [EditorLocalReplacement]
  ) -> [TextEdit]? {
    var result: [TextEdit] = []
    for replacement in replacements {
      guard replacement.range.length > 0 || !replacement.text.isEmpty else { continue }
      guard
        let start = position(
          source: source, offset: replacement.range.location, base: viewportStartLine),
        let end = position(
          source: source, offset: NSMaxRange(replacement.range), base: viewportStartLine)
      else { return nil }
      result.append(
        TextEdit(range: TextRange(start: start, end: end), text: replacement.text))
    }
    return result
  }

  private static func position(source: NSString, offset: Int, base: UInt32) -> TextPosition? {
    guard offset >= 0, offset <= source.length else { return nil }
    let prefix = source.substring(with: NSRange(location: 0, length: offset))
    let parts = prefix.components(separatedBy: "\n")
    guard let lineDelta = UInt32(exactly: parts.count - 1),
      let column = UInt32(exactly: parts.last?.utf16.count ?? 0)
    else { return nil }
    let line = base.addingReportingOverflow(lineDelta)
    guard !line.overflow else { return nil }
    return TextPosition(line: line.partialValue, column: column)
  }

  private static func validRanges(_ ranges: [NSRange], length: Int) -> [NSRange]? {
    let ordered = ranges.sorted { $0.location < $1.location }
    guard !ordered.isEmpty else { return nil }
    var end = 0
    for range in ordered {
      guard range.location != NSNotFound, range.location >= end, NSMaxRange(range) <= length else {
        return nil
      }
      end = NSMaxRange(range)
    }
    return ordered
  }

  private static func closingCharacter(_ inserted: String) -> String? {
    ["(": ")", "[": "]", "{": "}", "\"": "\"", "'": "'"][inserted]
  }

  private static func character(at offset: Int, in source: NSString) -> String? {
    guard offset < source.length else { return nil }
    return source.substring(with: source.rangeOfComposedCharacterSequence(at: offset))
  }

  private static func linePrefix(at offset: Int, in source: NSString) -> String {
    let line = source.lineRange(for: NSRange(location: offset, length: 0))
    return source.substring(with: NSRange(location: line.location, length: offset - line.location))
  }

  private static func indentation(at offset: Int, in source: NSString) -> String {
    String(linePrefix(at: offset, in: source).prefix { $0 == " " || $0 == "\t" })
  }

  private static func commentMarker(_ language: EditorLanguage) -> String {
    switch language {
    case .python, .shell: "#"
    case .plain_text, .rust, .swift, .typescript, .json, .markdown: "//"
    }
  }

  private static func lineStarts(_ source: NSString) -> [Int] {
    var starts = [0]
    for index in 0..<source.length where source.character(at: index) == 10 {
      starts.append(index + 1)
    }
    return starts
  }

  private static func selectedLineIndexes(_ range: NSRange, starts: [Int], length: Int) -> [Int] {
    let first = max(0, starts.lastIndex(where: { $0 <= range.location }) ?? 0)
    var endpoint = min(NSMaxRange(range), length)
    if range.length > 0, starts.contains(endpoint) { endpoint = max(range.location, endpoint - 1) }
    let last = max(first, starts.lastIndex(where: { $0 <= endpoint }) ?? first)
    return Array(first...last)
  }

  private struct LineDetail {
    let contentStart: Int
    let markerLength: Int
    let commented: Bool
  }

  private static func lineDetail(
    _ index: Int, starts: [Int], source: NSString, marker: String
  ) -> LineDetail {
    let start = starts[index]
    let limit = index + 1 < starts.count ? starts[index + 1] - 1 : source.length
    var content = start
    while content < limit, [9, 32].contains(source.character(at: content)) { content += 1 }
    let markerLength = marker.utf16.count
    let available = limit - content
    let commented =
      available >= markerLength
      && source.substring(with: NSRange(location: content, length: markerLength)) == marker
    var removal = markerLength
    if commented, content + removal < limit, source.character(at: content + removal) == 32 {
      removal += 1
    }
    return LineDetail(
      contentStart: content, markerLength: commented ? removal : 0, commented: commented)
  }

  private static func isWord(_ value: unichar) -> Bool {
    guard let scalar = Unicode.Scalar(Int(value)) else { return false }
    return value == 95 || CharacterSet.alphanumerics.contains(scalar)
  }
}
