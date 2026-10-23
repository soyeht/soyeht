import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func markdownPreviewUsesOnlyTheBoundedViewport() throws {
  let preview = try MarkdownViewportPreview(
    lines: ["# Heading", "[outside](https://example.invalid)", "<script>alert(1)</script>"],
    fromLine: 40, totalLineCount: 100_000)

  #expect(preview.fromLine == 40)
  #expect(preview.toLine == 43)
  #expect(preview.totalLineCount == 100_000)
  #expect(preview.isPartial)
  #expect(
    preview.blocks == [
      .heading(level: 1, [MarkdownRun("Heading")]),
      .paragraph([MarkdownRun("outside", link: "https://example.invalid")]),
      .paragraph([MarkdownRun("<script>alert(1)</script>")]),
    ])
}

/// The bug QA saw: the lines of a README ran together as "TitleSome bold text and a link.onetwo".
/// The preview parses with the agent replies' renderer, so a heading, a paragraph and a list stay
/// three blocks, with the styles of the text kept.
@Test func markdownPreviewKeepsTheBlocksOfADocument() throws {
  let preview = try MarkdownViewportPreview(
    lines: [
      "# Title", "Some **bold** text and a [link](https://example.com).", "- one", "- two",
    ], fromLine: 0, totalLineCount: 4)

  #expect(!preview.isPartial)
  #expect(
    preview.blocks == [
      .heading(level: 1, [MarkdownRun("Title")]),
      .paragraph([
        MarkdownRun("Some "), MarkdownRun("bold", bold: true),
        MarkdownRun(" text and a "), MarkdownRun("link", link: "https://example.com"),
        MarkdownRun("."),
      ]),
      .list(
        start: nil, items: [[.paragraph([MarkdownRun("one")])], [.paragraph([MarkdownRun("two")])]]),
    ])
}

@Test func markdownPreviewRefusesContentBeyondTheWireBudgets() throws {
  #expect(throws: MarkdownPreviewError.tooManyLines) {
    _ = try MarkdownViewportPreview(
      lines: Array(repeating: "line", count: ProtocolLimits.editorViewportLines + 1),
      fromLine: 0, totalLineCount: 10)
  }
  #expect(throws: MarkdownPreviewError.tooManyBytes) {
    _ = try MarkdownViewportPreview(
      lines: [String(repeating: "x", count: ProtocolLimits.jsonBytes + 1)],
      fromLine: 0, totalLineCount: 1)
  }
}

/// The cases of `markdownPreview.surfaces` in `contracts/vectors/editor.json`, which the desktop
/// client passes as well: a file's content is not trusted to open anything.
@Test func aLinkOpensOnlyWhereTheVectorSaysItDoes() throws {
  let vector = try ContractVector.load("editor")["markdownPreview"]
  let style = AgentConversationStyle.pane(
    SoyehtThemeColorSet(palette: TerminalColorTheme.neoMilk.palette))
  let runs = [MarkdownRun("docs", link: "https://example.com")]
  for item in ContractVector.cases(vector, "surfaces") {
    let name = item["name"].string ?? ""
    let surface: MarkdownSurface = item["surface"].string == "reply" ? .reply : .preview
    #expect(surface.opensAddresses == item["opensAddresses"].bool, "\(name)")
    #expect(surface.opensFiles == item["opensFiles"].bool, "\(name)")
    let look = MarkdownLook(
      workspaceId: surface == .reply ? "workspace" : nil, style: style, ink: style.ink)
    #expect(look.surface == surface, "\(name)")
    #expect(
      look.attributed(runs).runs.allSatisfy { $0.link != nil } == surface.opensAddresses,
      "\(name)")
  }
}
