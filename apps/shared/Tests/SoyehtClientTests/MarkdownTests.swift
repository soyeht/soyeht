import Foundation
import Testing

@testable import SoyehtClient

/// A vector's expected blocks, as `contracts/vectors/markdown.json` writes them.
private struct ExpectedBlock: Decodable {
  struct Run: Decodable {
    var text: String
    var bold: Bool?
    var italic: Bool?
    var strike: Bool?
    var code: Bool?
    var link: String?

    var run: MarkdownRun {
      MarkdownRun(
        text, bold: bold ?? false, italic: italic ?? false, strike: strike ?? false,
        code: code ?? false, link: link)
    }
  }

  var type: String
  var level: Int?
  var inlines: [Run]?
  var language: String?
  var text: String?
  var blocks: [ExpectedBlock]?
  var ordered: Bool?
  var start: Int?
  var items: [[ExpectedBlock]]?
  var header: [[Run]]?
  var rows: [[[Run]]]?

  var block: MarkdownBlock {
    let runs = (inlines ?? []).map(\.run)
    switch type {
    case "paragraph": return .paragraph(runs)
    case "heading": return .heading(level: level ?? 0, runs)
    case "code": return .code(language: language, text: text ?? "")
    case "quote": return .quote((blocks ?? []).map(\.block))
    case "list": return .list(start: start, items: (items ?? []).map { $0.map(\.block) })
    case "table":
      return .table(
        header: (header ?? []).map { $0.map(\.run) },
        rows: (rows ?? []).map { $0.map { $0.map(\.run) } })
    default: return .rule
    }
  }
}

private struct Vector: Decodable {
  var name: String
  var markdown: String
  var streaming: Bool
  var blocks: [ExpectedBlock]
}

private func vectors() throws -> [Vector] {
  let file = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
    .appendingPathComponent("contracts/vectors/markdown.json")
  return try JSONDecoder().decode([Vector].self, from: Data(contentsOf: file))
}

/// The rules every client shares: the desktop client's Rust parser passes the same file.
@Test func everySharedVectorParsesToItsBlocks() throws {
  let all = try vectors()
  #expect(all.count > 50)
  for vector in all {
    let parsed = MarkdownReply.parse(vector.markdown, streaming: vector.streaming)
    #expect(parsed == vector.blocks.map(\.block), "\(vector.name)")
  }
}

/// A reply arrives in pieces: no prefix of a realistic reply may fail to parse, and the finished
/// text parses to the same blocks whether or not it was streamed.
@Test func everyPrefixOfAReplyParsesWhileStreaming() throws {
  let reply = try #require(try vectors().first { $0.name == "realistic reply" }).markdown
  var prefix = ""
  for character in reply {
    prefix.append(character)
    _ = MarkdownReply.parse(prefix, streaming: true)
  }
  #expect(
    MarkdownReply.parse(reply, streaming: true) == MarkdownReply.parse(reply, streaming: false))
}
