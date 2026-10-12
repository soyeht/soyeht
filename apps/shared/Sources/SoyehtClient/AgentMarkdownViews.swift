import Foundation
import SwiftUI

/// An agent's words with their Markdown drawn: paragraphs, headings, lists, quotes, tables, rules
/// and code in a mono well, in the approved flat interior (palette roles only, no shadow). A reply
/// that is still arriving is parsed with its open spans closed, so a bold word or a code span
/// shows its style as it is typed.
struct AgentMarkdownText: View {
  let text: String
  let streaming: Bool
  let workspaceId: String
  let style: AgentConversationStyle
  var ink: Color?

  var body: some View {
    AgentMarkdownBlocks(
      blocks: MarkdownReplyCache.blocks(for: text, streaming: streaming), workspaceId: workspaceId,
      style: style, ink: ink ?? style.ink)
  }
}

private struct AgentMarkdownBlocks: View {
  let blocks: [MarkdownBlock]
  let workspaceId: String
  let style: AgentConversationStyle
  let ink: Color

  var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
      ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
        blockView(block)
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  private func nested(_ blocks: [MarkdownBlock]) -> AgentMarkdownBlocks {
    AgentMarkdownBlocks(blocks: blocks, workspaceId: workspaceId, style: style, ink: ink)
  }

  private func inline(_ runs: [MarkdownRun], bold: Bool = false) -> some View {
    FilePathText(
      MarkdownAttributedString.make(runs, style: style, bold: bold), workspaceId: workspaceId
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
      FilePathText(text, workspaceId: workspaceId)
        .font(style.monoFont).foregroundStyle(ink).tint(style.accent).textSelection(.enabled)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(PaneInteriorMetrics.rowSpacing)
        .paneInteriorFill(style.well, radius: PaneInteriorMetrics.wellRadius)
    case .quote(let blocks):
      HStack(alignment: .top, spacing: PaneInteriorMetrics.rowSpacing) {
        Rectangle().fill(style.well).frame(width: PaneInteriorMetrics.tightSpacing)
        AgentMarkdownBlocks(
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

/// A line of runs as one `AttributedString`: the bold, italic, struck, code and link looks (a link
/// takes the `tint` the view sets to the accent).
enum MarkdownAttributedString {
  static func make(_ runs: [MarkdownRun], style: AgentConversationStyle, bold: Bool = false)
    -> AttributedString
  {
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
        piece.link = url
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
