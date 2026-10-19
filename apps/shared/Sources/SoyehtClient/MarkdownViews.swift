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

/// What a block is drawn with. A block redraws when it, or this, changes. Without a
/// `workspaceId` the text is inert (see `MarkdownBlocksView`).
struct MarkdownLook: Equatable, Sendable {
  let workspaceId: String?
  let style: AgentConversationStyle
  let ink: Color

  /// A line of runs in this look: its links open only with a workspace.
  func attributed(_ runs: [MarkdownRun], bold: Bool = false) -> AttributedString {
    MarkdownAttributedString.make(runs, style: style, bold: bold, opensLinks: workspaceId != nil)
  }

  /// Text in this look: file references and web addresses open through the environment's
  /// actions when there is a workspace, and nothing opens without one.
  @ViewBuilder func drawn(_ text: AttributedString) -> some View {
    if let workspaceId {
      FilePathText(text, workspaceId: workspaceId)
    } else {
      Text(text)
    }
  }
}

/// Parsed Markdown drawn once for every pane that shows it: paragraphs, headings, lists, quotes,
/// tables, rules and code in a mono well, in the approved flat interior (palette roles only, no
/// shadow). The agent conversation and the editor's Markdown preview both draw through it.
///
/// With a `workspaceId`, file references and web addresses in the text open through the
/// environment's actions. Without one the text is inert: links keep their look (the accent,
/// underlined) and open nothing, which is how the preview treats a file's untrusted content.
///
/// Each block is `Equatable`, so while a reply streams only the block that grew runs its body and
/// is measured again; the finished blocks above it keep the layout they already have.
public struct MarkdownBlocksView: View {
  let blocks: [MarkdownBlock]
  let look: MarkdownLook
  /// The outermost block list owns text selection for everything nested in it.
  var isRoot = true

  public init(
    blocks: [MarkdownBlock], workspaceId: String? = nil, style: AgentConversationStyle,
    ink: Color? = nil
  ) {
    self.blocks = blocks
    look = MarkdownLook(workspaceId: workspaceId, style: style, ink: ink ?? style.ink)
  }

  fileprivate init(blocks: [MarkdownBlock], look: MarkdownLook) {
    self.blocks = blocks
    self.look = look
    isRoot = false
  }

  public var body: some View {
    VStack(alignment: .leading, spacing: PaneInteriorMetrics.compactSpacing) {
      ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
        MarkdownBlockView(block: block, look: look).equatable()
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
    .modifier(MarkdownSelection(isRoot: isRoot))
  }
}

private struct MarkdownBlockView: View, Equatable {
  let block: MarkdownBlock
  let look: MarkdownLook

  nonisolated static func == (lhs: Self, rhs: Self) -> Bool {
    lhs.block == rhs.block && lhs.look == rhs.look
  }

  private var style: AgentConversationStyle { look.style }

  private func nested(_ blocks: [MarkdownBlock], ink: Color? = nil) -> some View {
    MarkdownBlocksView(
      blocks: blocks,
      look: MarkdownLook(workspaceId: look.workspaceId, style: style, ink: ink ?? look.ink))
  }

  private func line(_ runs: [MarkdownRun], bold: Bool = false, fills: Bool = false) -> some View {
    MarkdownLineView(runs: runs, bold: bold, fills: fills, look: look).equatable()
  }

  /// On the Mac a paragraph is drawn a line at a time. A line break in a reply is a hard break,
  /// so this looks the same as one text, but a long paragraph that gains a line measures that
  /// line and not all the lines before it. iOS selects text per `Text`, so there a paragraph
  /// stays one text and can be selected whole.
  @ViewBuilder private func paragraph(_ runs: [MarkdownRun]) -> some View {
    #if os(macOS)
      let lines = MarkdownReply.lines(of: runs)
      if lines.count <= 1 {
        line(runs)
      } else {
        VStack(alignment: .leading, spacing: 0) {
          ForEach(Array(lines.enumerated()), id: \.offset) { _, runs in line(runs) }
        }
      }
    #else
      line(runs)
    #endif
  }

  var body: some View {
    switch block {
    case .paragraph(let runs): paragraph(runs)
    case .heading(_, let runs): line(runs, bold: true)
    case .code(_, let text):
      look.drawn(AttributedString(text))
        .font(style.monoFont).foregroundStyle(look.ink).tint(style.accent)
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(PaneInteriorMetrics.rowSpacing)
        .paneInteriorFill(style.well, radius: PaneInteriorMetrics.wellRadius)
    case .quote(let blocks):
      HStack(alignment: .top, spacing: PaneInteriorMetrics.rowSpacing) {
        Rectangle().fill(style.well).frame(width: PaneInteriorMetrics.tightSpacing)
        nested(blocks, ink: style.secondary)
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
          ForEach(Array(header.enumerated()), id: \.offset) { _, cell in
            line(cell, bold: true, fills: true)
          }
        }
        Rectangle().fill(style.well).frame(height: PaneInteriorMetrics.separatorWidth)
          .gridCellColumns(max(header.count, 1))
        ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
          GridRow {
            ForEach(Array(row.enumerated()), id: \.offset) { _, cell in line(cell, fills: true) }
          }
        }
      }
    case .rule:
      Rectangle().fill(style.well).frame(height: PaneInteriorMetrics.separatorWidth)
    }
  }
}

/// One line of runs. A stack gives its text the full width and the leading edge already, so a
/// line needs no frame of its own; a table cell does, to share the row's width with its
/// neighbours.
private struct MarkdownLineView: View, Equatable {
  let runs: [MarkdownRun]
  let bold: Bool
  let fills: Bool
  let look: MarkdownLook

  nonisolated static func == (lhs: Self, rhs: Self) -> Bool {
    lhs.runs == rhs.runs && lhs.bold == rhs.bold && lhs.fills == rhs.fills && lhs.look == rhs.look
  }

  @ViewBuilder var body: some View {
    let text = look.drawn(look.attributed(runs, bold: bold))
      .foregroundStyle(look.ink).tint(look.style.accent)
    if fills { text.frame(maxWidth: .infinity, alignment: .leading) } else { text }
  }
}

private struct MarkdownSelection: ViewModifier {
  let isRoot: Bool

  func body(content: Content) -> some View {
    if isRoot { content.textSelection(.enabled) } else { content }
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
