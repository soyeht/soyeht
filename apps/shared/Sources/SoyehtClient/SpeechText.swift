import Foundation

/// What of an agent's reply is spoken, and the sentences it is spoken in (ADR 0166 section 5).
/// The rules are in `contracts/vectors/speech-text.json`, which the Windows and Linux app passes too.
public enum SpeechText {
  /// The reply as plain lines to read aloud, one per paragraph, heading or list item, joined by
  /// newlines. Written text that is for the eyes is left out:
  /// - code blocks and tables are not read, and neither is struck-out text or a rule;
  /// - a link is read as its words, never its address, and a bare web address is dropped;
  /// - emoji and other pictographs are dropped, and a line with no letter or digit left with them;
  /// - a task-list marker is dropped, and blanks are collapsed.
  public static func speakable(_ markdown: String, streaming: Bool = false) -> String {
    lines(of: MarkdownReply.parse(markdown, streaming: streaming)).joined(separator: "\n")
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

  private static func spoken(_ runs: [MarkdownRun]) -> String {
    runs.filter { !$0.strike }.map(\.text).joined()
  }

  private static func clean(_ source: String) -> String? {
    var scalars = String.UnicodeScalarView()
    scalars.append(contentsOf: source.unicodeScalars.filter { !isPictograph($0) })
    var text = String(scalars)
    if let marker = ["[ ] ", "[x] ", "[X] "].first(where: text.hasPrefix) {
      text.removeFirst(marker.count)
    }
    var words = text.split(whereSeparator: \.isWhitespace).map(String.init)
    words.removeAll(where: isWebAddress)
    let line = words.joined(separator: " ")
    return line.contains(where: { $0.isLetter || $0.isNumber }) ? line : nil
  }

  private static func isWebAddress(_ word: String) -> Bool {
    let lower = word.lowercased()
    return ["http://", "https://", "www."].contains(where: lower.hasPrefix)
  }

  /// Emoji and symbols that a voice reads out as their names, and the joiners between them.
  private static func isPictograph(_ scalar: Unicode.Scalar) -> Bool {
    switch scalar.value {
    case 0x1F000...0x1FAFF, 0x2600...0x27BF, 0x2B00...0x2BFF, 0x200D, 0xFE0F: true
    default: false
    }
  }
}
