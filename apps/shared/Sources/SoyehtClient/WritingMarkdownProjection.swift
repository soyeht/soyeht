import Foundation

public enum WritingMarkdownLineKind: Equatable, Sendable {
  case blank
  case heading(level: Int)
  case paragraph
  case listItem(depth: Int, ordered: Bool)
  case quote
  case codeFence
  case code
  case table
  case image
}

public enum WritingMarkdownSpanKind: Equatable, Sendable {
  case strong
  case emphasis
  case inlineCode
  case link
  case syntax
}

public struct WritingMarkdownSpan: Equatable, Sendable {
  public let location: Int
  public let length: Int
  public let kind: WritingMarkdownSpanKind

  public init(location: Int, length: Int, kind: WritingMarkdownSpanKind) {
    self.location = location
    self.length = length
    self.kind = kind
  }
}

public struct WritingMarkdownLine: Equatable, Sendable {
  public let line: UInt32
  public let range: NSRange
  public let kind: WritingMarkdownLineKind
  public let rawSyntax: Bool

  public init(line: UInt32, range: NSRange, kind: WritingMarkdownLineKind, rawSyntax: Bool) {
    self.line = line
    self.range = range
    self.kind = kind
    self.rawSyntax = rawSyntax
  }
}

public struct WritingMarkdownProjection: Equatable, Sendable {
  public let text: String
  public let lines: [WritingMarkdownLine]
  public let spans: [WritingMarkdownSpan]

  public init(text: String, lines: [WritingMarkdownLine], spans: [WritingMarkdownSpan]) {
    self.text = text
    self.lines = lines
    self.spans = spans
  }

  public static func render(
    source: String, firstLine: UInt32 = 0, activeLine: UInt32
  ) -> WritingMarkdownProjection {
    let sourceLines = source.components(separatedBy: "\n")
    let tableLines = tableLineIndexes(sourceLines)
    let display = NSMutableString(string: source)
    var lines: [WritingMarkdownLine] = []
    var spans: [WritingMarkdownSpan] = []
    var offset = 0
    var inFence = false
    for (index, sourceLine) in sourceLines.enumerated() {
      let absolute = firstLine &+ UInt32(clamping: index)
      let raw = absolute == activeLine
      let classified = classify(sourceLine, table: tableLines.contains(index), inFence: &inFence)
      let length = sourceLine.utf16.count
      let range = NSRange(location: offset, length: length)
      lines.append(
        WritingMarkdownLine(line: absolute, range: range, kind: classified, rawSyntax: raw))
      projectLine(
        sourceLine, kind: classified, globalOffset: offset, raw: raw,
        display: display, spans: &spans)
      offset += length + (index + 1 < sourceLines.count ? 1 : 0)
    }
    return WritingMarkdownProjection(text: display as String, lines: lines, spans: spans)
  }
}

extension WritingMarkdownProjection {
  fileprivate static let strong = try? NSRegularExpression(
    pattern: #"\*\*([^*\n]+)\*\*|__([^_\n]+)__"#)
  fileprivate static let emphasis = try? NSRegularExpression(
    pattern: #"(?<!\*)\*([^*\n]+)\*(?!\*)|(?<!_)_([^_\n]+)_(?!_)"#)
  fileprivate static let code = try? NSRegularExpression(pattern: #"`([^`\n]+)`"#)
  fileprivate static let link = try? NSRegularExpression(
    pattern: #"(!?)\[([^\]\n]+)\]\(([^)\n]+)\)"#)

  fileprivate static func classify(
    _ line: String, table: Bool, inFence: inout Bool
  ) -> WritingMarkdownLineKind {
    let trimmed = line.trimmingCharacters(in: .whitespaces)
    if trimmed.hasPrefix("```") || trimmed.hasPrefix("~~~") {
      inFence.toggle()
      return .codeFence
    }
    if inFence { return .code }
    if trimmed.isEmpty { return .blank }
    if let heading = headingPrefix(line) { return .heading(level: heading.level) }
    if trimmed.hasPrefix("![") { return .image }
    if quoteMarker(line) != nil { return .quote }
    if let list = listMarker(line) { return .listItem(depth: list.depth, ordered: list.ordered) }
    if table { return .table }
    return .paragraph
  }

  fileprivate static func projectLine(
    _ line: String, kind: WritingMarkdownLineKind, globalOffset: Int, raw: Bool,
    display: NSMutableString, spans: inout [WritingMarkdownSpan]
  ) {
    if raw { return }
    switch kind {
    case .heading:
      if let prefix = headingPrefix(line) { conceal(prefix.range, at: globalOffset, in: display) }
    case .quote:
      if let marker = quoteMarker(line) {
        replace(marker, at: globalOffset, with: "│", in: display)
      }
    case .listItem(_, let ordered):
      if let marker = listMarker(line) {
        if ordered {
          conceal(marker.marker, at: globalOffset, in: display)
          replace(
            NSRange(location: marker.marker.location, length: 1), at: globalOffset, with: "•",
            in: display)
        } else {
          replace(marker.marker, at: globalOffset, with: "•", in: display)
        }
      }
    case .codeFence:
      conceal(NSRange(location: 0, length: line.utf16.count), at: globalOffset, in: display)
    case .table:
      renderTableMarkers(line, at: globalOffset, in: display)
    case .blank, .paragraph, .code, .image:
      break
    }
    inlineSpans(line, at: globalOffset, display: display, spans: &spans)
  }

  fileprivate static func inlineSpans(
    _ line: String, at offset: Int, display: NSMutableString,
    spans: inout [WritingMarkdownSpan]
  ) {
    appendMatches(strong, in: line, at: offset, kind: .strong, display: display, spans: &spans)
    appendMatches(
      emphasis, in: line, at: offset, kind: .emphasis, display: display, spans: &spans)
    appendMatches(code, in: line, at: offset, kind: .inlineCode, display: display, spans: &spans)
    appendLinkMatches(line, at: offset, display: display, spans: &spans)
  }

  fileprivate static func appendMatches(
    _ expression: NSRegularExpression?, in line: String, at offset: Int,
    kind: WritingMarkdownSpanKind, display: NSMutableString,
    spans: inout [WritingMarkdownSpan]
  ) {
    guard let expression else { return }
    let full = NSRange(location: 0, length: line.utf16.count)
    for match in expression.matches(in: line, range: full) {
      let content = firstCapture(in: match)
      guard content.location != NSNotFound else { continue }
      spans.append(
        WritingMarkdownSpan(location: offset + content.location, length: content.length, kind: kind)
      )
      concealOutside(content, within: match.range, at: offset, in: display)
    }
  }

  fileprivate static func appendLinkMatches(
    _ line: String, at offset: Int, display: NSMutableString,
    spans: inout [WritingMarkdownSpan]
  ) {
    guard let link else { return }
    let full = NSRange(location: 0, length: line.utf16.count)
    for match in link.matches(in: line, range: full) {
      let label = match.range(at: 2)
      guard label.location != NSNotFound else { continue }
      spans.append(
        WritingMarkdownSpan(location: offset + label.location, length: label.length, kind: .link))
      concealOutside(label, within: match.range, at: offset, in: display)
    }
  }

  fileprivate static func firstCapture(in match: NSTextCheckingResult) -> NSRange {
    for index in 1..<match.numberOfRanges {
      let range = match.range(at: index)
      if range.location != NSNotFound { return range }
    }
    return NSRange(location: NSNotFound, length: 0)
  }

  fileprivate static func concealOutside(
    _ content: NSRange, within whole: NSRange, at offset: Int, in display: NSMutableString
  ) {
    conceal(
      NSRange(location: whole.location, length: content.location - whole.location), at: offset,
      in: display)
    let trailing = NSMaxRange(whole) - NSMaxRange(content)
    conceal(NSRange(location: NSMaxRange(content), length: trailing), at: offset, in: display)
  }

  fileprivate static func conceal(_ range: NSRange, at offset: Int, in display: NSMutableString) {
    guard range.length > 0 else { return }
    display.replaceCharacters(
      in: NSRange(location: offset + range.location, length: range.length),
      with: String(repeating: "\u{200B}", count: range.length))
  }

  fileprivate static func replace(
    _ range: NSRange, at offset: Int, with value: String, in display: NSMutableString
  ) {
    guard range.length == value.utf16.count else { return }
    display.replaceCharacters(
      in: NSRange(location: offset + range.location, length: range.length), with: value)
  }

  fileprivate static func headingPrefix(_ line: String) -> (level: Int, range: NSRange)? {
    let values = Array(line.utf8)
    var index = 0
    while index < min(values.count, 3), values[index] == 32 { index += 1 }
    let start = index
    while index < values.count, values[index] == 35, index - start < 6 { index += 1 }
    guard index > start, index < values.count, values[index] == 32 else { return nil }
    let level = index - start
    while index < values.count, values[index] == 32 { index += 1 }
    return (level, NSRange(location: start, length: index - start))
  }

  fileprivate static func quoteMarker(_ line: String) -> NSRange? {
    let values = Array(line.utf8)
    var index = 0
    while index < values.count, values[index] == 32 { index += 1 }
    guard index < values.count, values[index] == 62 else { return nil }
    return NSRange(location: index, length: 1)
  }

  fileprivate static func listMarker(_ line: String) -> (
    marker: NSRange, depth: Int, ordered: Bool
  )? {
    let values = Array(line.utf8)
    var index = 0
    while index < values.count, values[index] == 32 { index += 1 }
    let depth = index / 2
    guard index < values.count else { return nil }
    if [42, 43, 45].contains(values[index]), index + 1 < values.count, values[index + 1] == 32 {
      return (NSRange(location: index, length: 1), depth, false)
    }
    let start = index
    while index < values.count, values[index] >= 48, values[index] <= 57 { index += 1 }
    guard index > start, index + 1 < values.count, values[index] == 46, values[index + 1] == 32
    else {
      return nil
    }
    return (NSRange(location: start, length: index - start + 1), depth, true)
  }

  fileprivate static func tableLineIndexes(_ lines: [String]) -> Set<Int> {
    var result: Set<Int> = []
    for index in lines.indices where isTableSeparator(lines[index]) {
      result.insert(index)
      if index > 0, lines[index - 1].contains("|") { result.insert(index - 1) }
      if index + 1 < lines.count, lines[index + 1].contains("|") { result.insert(index + 1) }
    }
    return result
  }

  fileprivate static func isTableSeparator(_ line: String) -> Bool {
    let cells = line.split(separator: "|", omittingEmptySubsequences: true)
    guard cells.count >= 2 else { return false }
    return cells.allSatisfy { cell in
      let value = cell.trimmingCharacters(in: .whitespaces).trimmingCharacters(
        in: CharacterSet(charactersIn: ":"))
      return value.count >= 3 && value.allSatisfy { $0 == "-" }
    }
  }

  fileprivate static func renderTableMarkers(
    _ line: String, at offset: Int, in display: NSMutableString
  ) {
    let separator = isTableSeparator(line)
    for (index, value) in line.utf16.enumerated() {
      if value == 124 {
        replace(
          NSRange(location: index, length: 1), at: offset, with: separator ? "┼" : "│", in: display)
      } else if separator, value == 45 {
        replace(NSRange(location: index, length: 1), at: offset, with: "─", in: display)
      } else if separator, value == 58 {
        conceal(NSRange(location: index, length: 1), at: offset, in: display)
      }
    }
  }
}
