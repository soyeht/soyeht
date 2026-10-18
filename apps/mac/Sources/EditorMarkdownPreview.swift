import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct EditorMarkdownPreview: View {
  let viewport: FileReadResult

  var body: some View {
    let preview = try? MarkdownViewportPreview(
      lines: viewport.lines.map(\.text), fromLine: viewport.fromLine,
      totalLineCount: viewport.file.lineCount)
    return VStack(alignment: .leading, spacing: 0) {
      HStack(spacing: MacTheme.Space.x6) {
        Text("PREVIEW")
          .font(MacTheme.ui(MacTheme.TypeScale.micro, weight: .bold))
          .tracking(0.8).foregroundStyle(MacTheme.secondary)
        Spacer(minLength: 0)
        if let preview, preview.isPartial {
          Text(rangeLabel(preview))
            .font(MacTheme.mono(MacTheme.TypeScale.micro)).foregroundStyle(MacTheme.muted)
        }
      }
      .padding(.horizontal, MacTheme.Space.x10).frame(height: 26)
      .background(MacTheme.canvas)

      if let preview {
        ScrollView {
          MarkdownBlocksView(blocks: preview.blocks, style: MacTheme.agentConversation)
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .padding(MacTheme.Space.x14)
        }
      } else {
        Text("This viewport cannot be previewed.")
          .font(MacTheme.ui(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary)
          .frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    }
    .frame(minWidth: 260, idealWidth: 360, maxWidth: 460, maxHeight: .infinity)
    .background(MacTheme.face)
    .accessibilityIdentifier("editor-markdown-preview")
  }

  private func rangeLabel(_ preview: MarkdownViewportPreview) -> String {
    let first = UInt64(preview.fromLine) + 1
    return "LINES \(first)–\(preview.toLine) OF \(preview.totalLineCount)"
  }
}
