import Foundation
import Testing

@testable import SoyehtClient

@Test func writingProjectionKeepsUtf16CoordinatesWhileHidingInactiveSyntax() {
  let source = "# Dawn\nA **bright** 😀 day\n> quiet"
  let projection = WritingMarkdownProjection.render(source: source, activeLine: 1)

  #expect(projection.text.utf16.count == source.utf16.count)
  #expect(projection.lines.map(\.kind) == [.heading(level: 1), .paragraph, .quote])
  #expect(projection.lines.filter(\.rawSyntax).map(\.line) == [1])
  #expect(!projection.text.hasPrefix("#"))
  #expect(projection.text.contains("**bright**"))
  #expect(projection.text.contains("│ quiet"))
}

@Test func writingProjectionRendersRichBlocksAndExposesOnlyTheActiveLineMarkup() {
  let source = """
    ## Heading
      - nested **item**
      12. ordered item
    | Name | Value |
    | --- | --- |
    | one | two |
    ```swift
    let value = 1
    ```
    ![cover](cover.png)
    """
  let projection = WritingMarkdownProjection.render(source: source, firstLine: 10, activeLine: 17)

  #expect(
    projection.lines.map(\.kind) == [
      .heading(level: 2), .listItem(depth: 1, ordered: false),
      .listItem(depth: 1, ordered: true), .table, .table, .table, .codeFence, .code,
      .codeFence, .image,
    ])
  #expect(projection.lines.filter(\.rawSyntax).map(\.line) == [17])
  #expect(projection.text.contains("• nested"))
  #expect(projection.text.contains("•\u{200B}\u{200B} ordered item"))
  #expect(!projection.text.contains("**item**"))
  #expect(projection.text.contains("│ Name │ Value │"))
  #expect(!projection.text.contains("```swift"))
  #expect(projection.text.contains("cover"))
  #expect(!projection.text.contains("cover.png"))
  #expect(projection.spans.contains { $0.kind == .strong })
}
