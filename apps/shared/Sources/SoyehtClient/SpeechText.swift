import Foundation

/// What of an agent's reply is spoken (ADR 0166 section 5). The rules are in
/// `contracts/vectors/speech-text.json`, which the Windows and Linux app passes too.
public enum SpeechText {
  /// The reply as plain lines to read aloud, one per paragraph, heading or list item, joined by
  /// newlines. Written text that is for the eyes is left out:
  /// - code blocks and tables are not read, and neither is struck-out text or a rule;
  /// - a link is read as its words, never its address; a bare web address is dropped, and a stop
  ///   that ended its sentence stays;
  /// - HTML tags are dropped, and so are emoji and symbols a voice would read out as their names
  ///   (pictographs, arrows, hourglasses, the keycap mark), and a line with no letter or digit
  ///   left with them;
  /// - a task-list marker is dropped, and blanks are collapsed.
  ///
  /// For a reply that is still arriving (`streaming`):
  /// - what an unfinished construct would still change is held back: from an unclosed `[` (a link
  ///   whose words or address are not complete) or an unmatched single `*` to the end, so that the
  ///   text spoken so far is always the start of the text to come;
  /// - the text ends with a newline when its last line is finished: the next block has begun, or
  ///   the reply ends with a blank line.
  public static func speakable(_ markdown: String, streaming: Bool = false) -> String {
    let source = streaming ? heldBack(markdown) : markdown
    let blocks = MarkdownReply.parse(source, streaming: streaming)
    var lines = lines(of: blocks)
    guard streaming, !lines.isEmpty else { return lines.joined(separator: "\n") }
    if lastLineIsFinished(blocks, source: source) { lines.append("") }
    return lines.joined(separator: "\n")
  }

  static func lines(of blocks: [MarkdownBlock]) -> [String] {
    blocks.flatMap(lines(of:))
  }

  private static func lines(of block: MarkdownBlock) -> [String] {
    switch block {
    case .paragraph(let runs), .heading(_, let runs): clean(spoken(runs)).map { [$0] } ?? []
    case .quote(let blocks): lines(of: blocks)
    case .list(_, let items): items.flatMap(lines(of:))
    case .code, .table, .rule: []
    }
  }

  /// The last spoken line is complete when a block follows it, or the text ends in a blank line.
  private static func lastLineIsFinished(_ blocks: [MarkdownBlock], source: String) -> Bool {
    if let last = blocks.last, case .code = last { return true }
    if let last = blocks.last, case .table = last { return true }
    if let last = blocks.last, case .rule = last { return true }
    var newlines = 0
    for character in source.reversed() {
      if character == "\n" {
        newlines += 1
      } else if character != " " && character != "\t" {
        break
      }
    }
    return newlines >= 2
  }

  /// The markdown cut where an unfinished link or italic begins in its last line. Inside an open
  /// code fence nothing is held: code is not read.
  private static func heldBack(_ markdown: String) -> String {
    let fences = markdown.split(separator: "\n", omittingEmptySubsequences: false)
      .filter { $0.hasPrefix("```") || $0.hasPrefix("~~~") }
    guard fences.count.isMultiple(of: 2) else { return markdown }
    let start =
      markdown.lastIndex(of: "\n").map { markdown.index(after: $0) } ?? markdown.startIndex
    let line = markdown[start...]
    var open: String.Index?
    var star: String.Index?
    var inCode = false
    var index = line.startIndex
    var previous: Character = "\n"
    while index < line.endIndex {
      let character = line[index]
      let next = line.index(after: index)
      switch character {
      case "`": inCode.toggle()
      case "[" where !inCode: open = open ?? index
      case "]" where !inCode:
        if next < line.endIndex, line[next] == "(" {
          if let close = line[next...].firstIndex(of: ")") {
            index = close
            open = nil
          }
        } else {
          open = nil
        }
      case "*" where !inCode:
        if next < line.endIndex, line[next] == "*" {
          index = next
        } else if next < line.endIndex, line[next].isWhitespace, previous.isWhitespace {
          // A bullet, or a star set apart by blanks: not emphasis.
        } else {
          star = star == nil ? index : nil
        }
      default: break
      }
      previous = line[index]
      index = line.index(after: index)
    }
    let cut = [open, star].compactMap { $0 }.min()
    return cut.map { String(markdown[..<$0]) } ?? markdown
  }

  private static func spoken(_ runs: [MarkdownRun]) -> String {
    runs.filter { !$0.strike }.map(\.text).joined()
  }

  private static func clean(_ source: String) -> String? {
    var text = String(String.UnicodeScalarView(source.unicodeScalars.filter { !isPictograph($0) }))
    text = text.replacingOccurrences(
      of: "<[A-Za-z/!][^>]*>", with: "", options: .regularExpression)
    if let marker = ["[ ] ", "[x] ", "[X] "].first(where: text.hasPrefix) {
      text.removeFirst(marker.count)
    }
    var words: [String] = []
    for word in text.split(whereSeparator: \.isWhitespace).map(String.init) {
      guard isWebAddress(word) else {
        words.append(word)
        continue
      }
      // The address is not read; the stop that ended the sentence it was in stays.
      let stop = word.reversed().prefix { ".!?…".contains($0) }
      if !stop.isEmpty, !words.isEmpty { words[words.count - 1] += String(stop.reversed()) }
    }
    let line = words.joined(separator: " ")
    return line.contains(where: { $0.isLetter || $0.isNumber }) ? line : nil
  }

  private static func isWebAddress(_ word: String) -> Bool {
    let bare = word.drop { "([{<\"'".contains($0) }.lowercased()
    return ["http://", "https://", "www."].contains(where: bare.hasPrefix)
  }

  /// Emoji and symbols that a voice reads out as their names, and the joiners between them.
  private static func isPictograph(_ scalar: Unicode.Scalar) -> Bool {
    switch scalar.value {
    case 0x1F000...0x1FAFF, 0x2190...0x21FF, 0x2300...0x23FF, 0x2600...0x27BF, 0x2900...0x297F,
      0x2B00...0x2BFF, 0x20E3, 0x200D, 0xFE0F:
      true
    default: false
    }
  }
}
