import Foundation

/// Closes what a reply that is still arriving has left open.
///
/// "Here is **the" would show its asterisks until the closing pair arrives, then jump to bold.
/// Closing the open span first shows the style as the words are typed. Only the spans whose
/// delimiters cannot be mistaken for text are tracked: code spans (a run of backticks), bold
/// (`**`) and strikethrough (`~~`). A delimiter opens a span when the next character is not
/// whitespace and closes one when the previous character is not whitespace, as in CommonMark;
/// one that ends the text, with nothing after it yet, is dropped. An open code fence is left
/// alone (the parser already runs it to the end), and a blank line ends every open span.
/// `contracts/vectors/markdown.json` holds the cases the desktop client shares.
enum MarkdownStreaming {
  private enum Delimiter: Equatable {
    case code(Int)
    case bold
    case strike

    var text: String {
      switch self {
      case .code(let length): return String(repeating: "`", count: length)
      case .bold: return "**"
      case .strike: return "~~"
      }
    }
  }

  /// The delimiters that are open, each with where it began.
  private typealias Open = [(delimiter: Delimiter, begin: Int)]

  static func close(_ text: String) -> String {
    let characters = Array(text)
    var open: Open = []
    var fence: Character?
    var start = 0
    while start < characters.count {
      var end = start
      while end < characters.count, characters[end] != "\n" { end += 1 }
      let next = min(end + 1, characters.count)
      let line = characters[start..<next]
      let trimmed = line.drop(while: { $0 == " " || $0 == "\t" })
      let marker: Character? = ["`", "~"].first { trimmed.starts(with: [$0, $0, $0]) }
      switch (fence, marker) {
      case (let opened?, let marker?) where opened == marker:
        if trimmed.drop(while: { $0 == marker }).allSatisfy(\.isWhitespace) { fence = nil }
      case (.some, _): break
      case (nil, let marker?):
        open.removeAll()
        fence = marker
      case (nil, nil) where trimmed.allSatisfy(\.isWhitespace): open.removeAll()
      case (nil, nil): scan(characters, from: start, to: next, open: &open)
      }
      start = next
    }
    guard fence == nil else { return text }
    if let last = open.last, last.begin + last.delimiter.text.count == characters.count {
      open.removeLast()
      return String(characters[..<last.begin]) + open.reversed().map(\.delimiter.text).joined()
    }
    return text + open.reversed().map(\.delimiter.text).joined()
  }

  /// Tracks the delimiters of one line, `from` up to (not including) `to`.
  private static func scan(_ characters: [Character], from: Int, to: Int, open: inout Open) {
    var index = from
    while index < to {
      let current = characters[index]
      var run = 1
      while index + run < to, characters[index + run] == current { run += 1 }
      let before = index > from ? characters[index - 1] : nil
      let after = index + run < to ? characters[index + run] : nil
      let inCode: Bool
      if case .code? = open.last?.delimiter { inCode = true } else { inCode = false }
      if current == "`" {
        if case .code(let length)? = open.last?.delimiter {
          if length == run { open.removeLast() }
        } else {
          open.append((.code(run), index))
        }
        index += run
      } else if inCode {
        index += 1
      } else if current == "\\" {
        index += 2
      } else if (current == "*" || current == "~") && run == 2 {
        toggle(current == "*" ? .bold : .strike, at: index, before: before, after: after, &open)
        index += run
      } else {
        index += run
      }
    }
  }

  private static func toggle(
    _ delimiter: Delimiter, at index: Int, before: Character?, after: Character?, _ open: inout Open
  ) {
    if let position = open.lastIndex(where: { $0.delimiter == delimiter }) {
      if let before, !before.isWhitespace { open.remove(at: position) }
    } else if after.map({ !$0.isWhitespace }) ?? true {
      open.append((delimiter, index))
    }
  }
}
