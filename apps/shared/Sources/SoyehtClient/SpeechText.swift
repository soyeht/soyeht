import Foundation
import SoyehtProtocol

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
  /// - a task-list marker is dropped, and blanks are collapsed;
  /// - a line break of the reply, `\r\n` or a lone `\r`, is read as `\n` before anything else.
  ///
  /// For a reply that is still arriving (`streaming`):
  /// - what an unfinished construct would still change is held back: from an unclosed `[` (a link
  ///   whose words or address are not complete) or an unmatched single `*` to the end, so that the
  ///   text spoken so far is always the start of the text to come;
  /// - the text ends with a newline when its last line is finished: the next block has begun, or
  ///   the reply ends with a blank line.
  public static func speakable(_ markdown: String, streaming: Bool = false) -> String {
    let lineFeeds = markdown.replacingOccurrences(of: "\r\n", with: "\n")
      .replacingOccurrences(of: "\r", with: "\n")
    let source = streaming ? heldBack(lineFeeds) : lineFeeds
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
    var underscore: String.Index?
    var inCode = false
    var index = line.startIndex
    var previous: Character = "\n"
    while index < line.endIndex {
      let character = line[index]
      let next = line.index(after: index)
      let following: Character = next < line.endIndex ? line[next] : "\n"
      switch character {
      case "`": inCode.toggle()
      case "[" where !inCode: open = open ?? index
      case "]" where !inCode:
        if following == "(" {
          if let close = line[next...].firstIndex(of: ")") {
            index = close
            open = nil
          }
        } else {
          open = nil
        }
      case "*" where !inCode:
        if following == "*" {
          index = next
        } else if !(following.isWhitespace && previous.isWhitespace) {
          // A bullet, or a star set apart by blanks, is not emphasis.
          star = star == nil ? index : nil
        }
      case "_" where !inCode:
        if following == "_" {
          index = next
        } else if !(previous.isLetter || previous.isNumber)
          || !(following.isLetter || following.isNumber)
        {
          // An underscore inside a word (snake_case) is not emphasis.
          if !(following.isWhitespace && previous.isWhitespace) {
            underscore = underscore == nil ? index : nil
          }
        }
      default: break
      }
      previous = line[index]
      index = line.index(after: index)
    }
    let cut = [open, star, underscore].compactMap { $0 }.min()
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
      // The address is not read, and neither are the closing brackets and quotes after it; the
      // stop that ended the sentence it was in stays.
      let stop = word.reversed().drop { ClientPolicy.voiceSentenceClosers.contains($0) }.prefix {
        ClientPolicy.voiceSentenceStops.contains($0)
          || ClientPolicy.voiceSentenceWideStops.contains($0)
      }
      if !stop.isEmpty, !words.isEmpty { words[words.count - 1] += String(stop.reversed()) }
    }
    let line = words.joined(separator: " ")
    return line.contains(where: { $0.isLetter || $0.isNumber }) ? line : nil
  }

  private static let addressPrefixes = ClientPolicy.voiceAddressPrefixes.split(separator: "|").map(
    String.init)
  private static let pictographs = ScalarRanges(policy: ClientPolicy.voicePictographs)

  private static func isWebAddress(_ word: String) -> Bool {
    let bare = word.drop { ClientPolicy.voiceAddressOpeners.contains($0) }.lowercased()
    return addressPrefixes.contains(where: bare.hasPrefix)
  }

  /// Emoji and symbols that a voice reads out as their names, and the joiners between them.
  private static func isPictograph(_ scalar: Unicode.Scalar) -> Bool {
    pictographs.contains(scalar)
  }
}
