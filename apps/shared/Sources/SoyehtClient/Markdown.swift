import Foundation
import Markdown

/// Text in one style. Neighbouring runs of the same style are merged.
public struct MarkdownRun: Equatable, Sendable {
  public var text: String
  public var bold = false
  public var italic = false
  public var strike = false
  public var code = false
  public var link: String?

  public init(
    _ text: String, bold: Bool = false, italic: Bool = false, strike: Bool = false,
    code: Bool = false, link: String? = nil
  ) {
    self.text = text
    self.bold = bold
    self.italic = italic
    self.strike = strike
    self.code = code
    self.link = link
  }

  fileprivate func sameStyle(as other: MarkdownRun) -> Bool {
    bold == other.bold && italic == other.italic && strike == other.strike && code == other.code
      && link == other.link
  }
}

public indirect enum MarkdownBlock: Equatable, Sendable {
  case paragraph([MarkdownRun])
  case heading(level: Int, [MarkdownRun])
  case code(language: String?, text: String)
  case quote([MarkdownBlock])
  /// `start` is the first number of an ordered list, `nil` for a bullet list.
  case list(start: Int?, items: [[MarkdownBlock]])
  case table(header: [[MarkdownRun]], rows: [[[MarkdownRun]]])
  case rule
}

/// Agent replies as blocks: the Markdown subset every Soyeht client draws.
///
/// The rules live in `contracts/vectors/markdown.json`, which the desktop client's Rust parser
/// passes too (`MarkdownTests` here). swift-markdown (cmark-gfm) does the parsing; this maps its
/// tree onto the vector's shape: paragraphs, headings, fenced and indented code, block quotes,
/// bullet and ordered lists, tables and rules, with runs of text styled bold, italic, struck,
/// code or linked. A link is kept only for http, https and mailto addresses, and one whose text
/// is its own address is plain text (the pane linkifies bare addresses itself). Raw HTML stays
/// literal, and so does a task list's `[ ]` or `[x]`. A line break is a `\n` in the text. Empty
/// code blocks and empty list items are dropped.
///
/// Parsing is a pure function of the text, so a reply that is still arriving is re-parsed each
/// time it grows; `MarkdownStreaming.close` first closes what the reply has left open, so a bold
/// word or a code span shows its style as it is typed instead of showing raw markers.
public enum MarkdownReply {
  public static func parse(_ text: String, streaming: Bool = false) -> [MarkdownBlock] {
    let source = streaming ? MarkdownStreaming.close(text) : text
    return blocks(Document(parsing: source, options: [.disableSmartOpts]).children)
  }

  /// A paragraph's runs split at its line breaks, each break ending a line. A span that crosses
  /// a break keeps its style on both sides.
  public static func lines(of runs: [MarkdownRun]) -> [[MarkdownRun]] {
    var lines: [[MarkdownRun]] = [[]]
    for run in runs {
      let parts = run.text.split(separator: "\n", omittingEmptySubsequences: false)
      for (index, part) in parts.enumerated() {
        if index > 0 { lines.append([]) }
        guard !part.isEmpty else { continue }
        var piece = run
        piece.text = String(part)
        lines[lines.count - 1].append(piece)
      }
    }
    return lines.filter { !$0.isEmpty }
  }

  private static func blocks(_ children: some Sequence<Markup>) -> [MarkdownBlock] {
    children.compactMap(block)
  }

  private static func block(_ markup: Markup) -> MarkdownBlock? {
    switch markup {
    case let paragraph as Paragraph:
      let runs = inlines(paragraph)
      return runs.isEmpty ? nil : .paragraph(runs)
    case let heading as Heading:
      let runs = inlines(heading)
      return runs.isEmpty ? nil : .heading(level: heading.level, runs)
    case let code as CodeBlock:
      let text = code.code.hasSuffix("\n") ? String(code.code.dropLast()) : code.code
      let language = code.language?.split(whereSeparator: \.isWhitespace).first.map(String.init)
      return text.isEmpty ? nil : .code(language: language, text: text)
    case let quote as BlockQuote:
      return .quote(blocks(quote.children))
    case let list as UnorderedList:
      return items(of: list).map { .list(start: nil, items: $0) }
    case let list as OrderedList:
      return items(of: list).map { .list(start: Int(list.startIndex), items: $0) }
    case is ThematicBreak:
      return .rule
    case let table as Markdown.Table:
      return self.table(table)
    case let html as HTMLBlock:
      let text = html.rawHTML.trimmingCharacters(in: .newlines)
      return text.isEmpty ? nil : .paragraph([MarkdownRun(text)])
    default:
      return nil
    }
  }

  private static func items(of list: some ListItemContainer) -> [[MarkdownBlock]]? {
    let items: [[MarkdownBlock]] = Array(list.listItems).map { item -> [MarkdownBlock] in
      var content = blocks(item.children)
      if let box = item.checkbox {
        // A task list marker stays as typed, at the start of the item's first paragraph.
        let marker = MarkdownRun(box == .checked ? "[x] " : "[ ] ")
        if case .paragraph(let runs)? = content.first {
          content[0] = .paragraph(merged([marker], runs))
        } else {
          content.insert(.paragraph([marker]), at: 0)
        }
      }
      return content
    }.filter { !$0.isEmpty }
    return items.isEmpty ? nil : items
  }

  private static func table(_ table: Markdown.Table) -> MarkdownBlock? {
    let header: [[MarkdownRun]] = table.head.cells.map { inlines($0) }
    guard !header.isEmpty else { return nil }
    let rows: [[[MarkdownRun]]] = table.body.rows.map { row in
      var cells: [[MarkdownRun]] = row.cells.prefix(header.count).map { inlines($0) }
      while cells.count < header.count { cells.append([]) }
      return cells
    }
    return .table(header: header, rows: rows)
  }

  // MARK: Inline

  private struct Style {
    var bold = false
    var italic = false
    var strike = false
    var link: String?
  }

  private static func inlines(_ parent: Markup) -> [MarkdownRun] {
    var runs: [MarkdownRun] = []
    for child in parent.children { collect(child, Style(), into: &runs) }
    return runs
  }

  private static func collect(_ markup: Markup, _ style: Style, into runs: inout [MarkdownRun]) {
    func add(_ text: String, code: Bool = false) {
      guard !text.isEmpty else { return }
      let run = MarkdownRun(
        text, bold: style.bold, italic: style.italic, strike: style.strike, code: code,
        link: style.link)
      runs = merged(runs, [run])
    }
    switch markup {
    case let text as Markdown.Text: add(text.string)
    case let code as InlineCode: add(code.code, code: true)
    case is SoftBreak, is LineBreak: add("\n")
    case let html as InlineHTML: add(html.rawHTML)
    case let emphasis as Emphasis:
      var inner = style
      inner.italic = true
      for child in emphasis.children { collect(child, inner, into: &runs) }
    case let strong as Strong:
      var inner = style
      inner.bold = true
      for child in strong.children { collect(child, inner, into: &runs) }
    case let strike as Strikethrough:
      var inner = style
      inner.strike = true
      for child in strike.children { collect(child, inner, into: &runs) }
    case let link as Markdown.Link:
      linked(link, address: link.destination, style, into: &runs)
    case let image as Markdown.Image:
      linked(image, address: image.source, style, into: &runs)
    default:
      for child in markup.children { collect(child, style, into: &runs) }
    }
  }

  /// The children of a link or an image, linked to `address` when a pane may open it and the text
  /// is not the address itself.
  private static func linked(
    _ markup: Markup, address: String?, _ style: Style, into runs: inout [MarkdownRun]
  ) {
    var inner = style
    if let address, allowed(address), plainText(of: markup) != address { inner.link = address }
    for child in markup.children { collect(child, inner, into: &runs) }
  }

  private static func plainText(of markup: Markup) -> String {
    switch markup {
    case let text as Markdown.Text: return text.string
    case let code as InlineCode: return code.code
    default: return markup.children.map(plainText(of:)).joined()
    }
  }

  private static func allowed(_ address: String) -> Bool {
    let lower = address.lowercased()
    return ["http://", "https://", "mailto:"].contains(where: lower.hasPrefix)
  }

  fileprivate static func merged(_ runs: [MarkdownRun], _ added: [MarkdownRun]) -> [MarkdownRun] {
    var result = runs
    for run in added {
      if let last = result.last, last.sameStyle(as: run) {
        result[result.count - 1].text += run.text
      } else {
        result.append(run)
      }
    }
    return result
  }
}

/// The parse of every reply a transcript shows, kept by text. A transcript re-evaluates its rows
/// each time any message changes, and a reply that has not changed must not be parsed again.
enum MarkdownReplyCache {
  private final class Parsed {
    let blocks: [MarkdownBlock]
    init(_ blocks: [MarkdownBlock]) { self.blocks = blocks }
  }

  private nonisolated(unsafe) static let cache: NSCache<NSString, Parsed> = {
    let cache = NSCache<NSString, Parsed>()
    cache.countLimit = 512
    return cache
  }()

  /// How many replies were parsed rather than found, for the tests that bound the work.
  private(set) nonisolated(unsafe) static var parseCount = 0

  static func blocks(for text: String, streaming: Bool) -> [MarkdownBlock] {
    let key = (streaming ? "s" : "f") + text as NSString
    if let known = cache.object(forKey: key) { return known.blocks }
    parseCount += 1
    let parsed = MarkdownReply.parse(text, streaming: streaming)
    cache.setObject(Parsed(parsed), forKey: key)
    return parsed
  }
}
