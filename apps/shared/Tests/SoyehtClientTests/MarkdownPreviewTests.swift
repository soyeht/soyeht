import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func markdownPreviewUsesOnlyTheBoundedViewportAndDisablesLinks() throws {
  let preview = try MarkdownViewportPreview(
    lines: ["# Heading", "[outside](https://example.invalid)", "<script>alert(1)</script>"],
    fromLine: 40, totalLineCount: 100_000)

  #expect(preview.fromLine == 40)
  #expect(preview.toLine == 43)
  #expect(preview.totalLineCount == 100_000)
  #expect(preview.isPartial)
  #expect(String(preview.content.characters).contains("Heading"))
  #expect(String(preview.content.characters).contains("<script>alert(1)</script>"))
  #expect(!preview.content.runs.contains { $0.link != nil })
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
