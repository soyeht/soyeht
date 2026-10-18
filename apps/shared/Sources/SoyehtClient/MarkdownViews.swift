import Foundation
import SwiftUI

/// An agent's words with their Markdown drawn by `MarkdownBlocksView`. A reply that is still
/// arriving is parsed with its open spans closed, so a bold word or a code span shows its style as
/// it is typed.
struct AgentMarkdownText: View {
  let text: String
  let streaming: Bool
  let workspaceId: String
  let style: AgentConversationStyle
  var ink: Color?

  var body: some View {
    MarkdownBlocksView(
      blocks: MarkdownReplyCache.blocks(for: text, streaming: streaming), workspaceId: workspaceId,
      style: style, ink: ink ?? style.ink)
  }
}

/// Parsed Markdown drawn once for every pane that shows it: paragraphs, headings, lists, quotes,
/// tables, rules and code in a mono well, in the approved flat interior (palette roles only, no
/// shadow). The agent conversation and the editor's Markdown preview both draw through it.
///
/// With a `workspaceId`, file references and web addresses in the text open through the
/// environment's actions. Without one the text is inert: links keep their look (the accent,
/// underlined) and open nothing, which is how the preview treats a file's untrusted content.
public struct MarkdownBlocksView: View {
  let blocks: [MarkdownBlock]
  let workspaceId: String?
  let style: AgentConversationStyle
  let ink: Color

  public init(
    blocks: [MarkdownBlock], workspaceId: String? = nil, style: AgentConversationStyle,
    ink: Color? = nil
  ) {
    self.blocks = blocks
    self.workspaceId = workspaceId
    self.style = style
    self.ink = ink ?? style.ink
  }

  public var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
      ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
        blockView(block)
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  private func nested(_ blocks: [MarkdownBlock]) -> MarkdownBlocksView {
    MarkdownBlocksView(blocks: blocks, workspaceId: workspaceId, style: style, ink: ink)
  }

  @ViewBuilder private func drawn(_ text: AttributedString) -> some View {
    if let workspaceId {
      FilePathText(text, workspaceId: workspaceId)
    } else {
      Text(text)
    }
  }

  private func inline(_ runs: [MarkdownRun], bold: Bool = false) -> some View {
    drawn(
      MarkdownAttributedString.make(runs, style: style, bold: bold, opensLinks: workspaceId != nil)
    )
    .foregroundStyle(ink).tint(style.accent)
    .textSelection(.enabled)
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  @ViewBuilder private func blockView(_ block: MarkdownBlock) -> some View {
    switch block {
    case .paragraph(let runs): inline(runs)
    case .heading(_, let runs): inline(runs, bold: true)
    case .code(_, let text):
      drawn(AttributedString(text))
        .font(style.monoFont).foregroundStyle(ink).tint(style.accent).textSelection(.enabled)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(PaneInteriorMetrics.rowSpacing)
        .paneInteriorFill(style.well, radius: PaneInteriorMetrics.wellRadius)
    case .quote(let blocks):
      HStack(alignment: .top, spacing: PaneInteriorMetrics.rowSpacing) {
        Rectangle().fill(style.well).frame(width: PaneInteriorMetrics.tightSpacing)
        MarkdownBlocksView(
          blocks: blocks, workspaceId: workspaceId, style: style, ink: style.secondary)
      }
      .fixedSize(horizontal: false, vertical: true)
    case .list(let start, let items):
      VStack(alignment: .leading, spacing: PaneInteriorMetrics.tightSpacing) {
        ForEach(Array(items.enumerated()), id: \.offset) { index, item in
          HStack(alignment: .firstTextBaseline, spacing: PaneInteriorMetrics.rowSpacing) {
            Text(start.map { "\($0 + index)." } ?? "\u{2022}")
              .foregroundStyle(style.secondary)
              .frame(minWidth: PaneInteriorMetrics.cardPadding, alignment: .trailing)
            nested(item)
          }
        }
      }
    case .table(let header, let rows):
      Grid(
        alignment: .leading, horizontalSpacing: PaneInteriorMetrics.rowSpacing,
        verticalSpacing: PaneInteriorMetrics.tightSpacing
      ) {
        GridRow {
          ForEach(Array(header.enumerated()), id: \.offset) { _, cell in inline(cell, bold: true) }
        }
        Rectangle().fill(style.well).frame(height: PaneInteriorMetrics.separatorWidth)
          .gridCellColumns(max(header.count, 1))
        ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
          GridRow { ForEach(Array(row.enumerated()), id: \.offset) { _, cell in inline(cell) } }
        }
      }
    case .rule:
      Rectangle().fill(style.well).frame(height: PaneInteriorMetrics.separatorWidth)
    }
  }
}

/// A line of runs as one `AttributedString`: the bold, italic, struck, code and link looks. A link
/// that opens takes the `tint` the view sets to the accent; one that does not open carries the
/// accent and an underline itself.
enum MarkdownAttributedString {
  static func make(
    _ runs: [MarkdownRun], style: AgentConversationStyle, bold: Bool = false,
    opensLinks: Bool = true
  ) -> AttributedString {
    var result = AttributedString()
    for run in runs {
      var piece = AttributedString(run.text)
      if run.code {
        piece.font = Self.codeFont(style)
        piece.backgroundColor = style.well
      } else if run.bold || run.italic || bold {
        var font = style.bodyFont
        if run.bold || bold { font = font.bold() }
        if run.italic { font = font.italic() }
        piece.font = font
      }
      if run.strike { piece.strikethroughStyle = .single }
      if let link = run.link, let url = URL(string: link) {
        if opensLinks {
          piece.link = url
        } else {
          piece.foregroundColor = style.accent
          piece.underlineStyle = .single
        }
      }
      result += piece
    }
    return result
  }

  /// Inline code is drawn in the mono face at the size of the words around it.
  private static func codeFont(_ style: AgentConversationStyle) -> Font {
    style.canonicalLayout
      ? PaneInteriorFont.mono(PaneInteriorMetrics.bodyPointSize) : style.monoFont
  }
}
