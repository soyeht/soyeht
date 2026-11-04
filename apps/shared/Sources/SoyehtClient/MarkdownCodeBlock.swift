import SoyehtProtocol
import SwiftUI

/// A fenced or indented code block: the code in a mono well and, at the top right corner, a flat
/// Copy button. The button is a column of its own at the trailing edge, so a long line wraps
/// before it and the button stays where it is. It copies `text` exactly as the parser kept it
/// (no fence, no language tag, the closing newline already dropped, inner whitespace intact) and
/// shows a check for `ClientPolicy.copiedFeedbackSeconds`.
struct MarkdownCodeBlockView: View {
  let text: String
  let look: MarkdownLook

  @State private var copied = false
  /// Counts the copies, so a second press restarts the check's timer.
  @State private var copies = 0

  private var style: AgentConversationStyle { look.style }

  /// A finger needs the full touch target; a pointer, a small button.
  private static var side: CGFloat {
    #if os(macOS)
      PaneInteriorMetrics.iconButtonSize
    #else
      PaneInteriorMetrics.touchTarget
    #endif
  }

  var body: some View {
    HStack(alignment: .top, spacing: 0) {
      look.drawn(AttributedString(text))
        .font(style.monoFont).foregroundStyle(look.ink).tint(style.accent)
        .padding(.vertical, PaneInteriorMetrics.rowSpacing)
        .padding(.leading, PaneInteriorMetrics.rowSpacing)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
      copyButton
    }
    .frame(minHeight: Self.side)
    .paneInteriorFill(style.well, radius: PaneInteriorMetrics.wellRadius)
    .task(id: copies) {
      guard copied else { return }
      do {
        try await Task.sleep(for: .seconds(ClientPolicy.copiedFeedbackSeconds))
        copied = false
      } catch {
        // Cancelled by the next copy or by the block leaving the screen: nothing to undo.
      }
    }
  }

  private var copyButton: some View {
    Button {
      SystemClipboard.copy(text)
      copied = true
      copies += 1
    } label: {
      Image(systemName: copied ? "checkmark" : "doc.on.doc")
        .font(style.captionFont)
        .foregroundStyle(copied ? style.success : style.secondary)
        .frame(width: Self.side, height: Self.side)
        .contentShape(Rectangle())
    }
    .buttonStyle(.plain)
    .accessibilityLabel(ClientPolicy.copyCode)
    .accessibilityValue(copied ? ClientPolicy.copiedCode : "")
    #if os(macOS)
      .help(ClientPolicy.copyCode)
    #endif
  }
}
