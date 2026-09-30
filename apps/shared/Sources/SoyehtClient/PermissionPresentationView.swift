import SoyehtProtocol
import SwiftUI

/// The typed body of a permission request: the command, tool arguments and file changes the
/// person approves. Every harness's request renders through this one view.
struct PermissionPresentationView: View {
  let presentation: PermissionPresentation
  let style: AgentConversationStyle

  var body: some View {
    VStack(alignment: .leading, spacing: 9) {
      if let command = presentation.command {
        PermissionTextBox(label: "Command", text: command, style: style)
      }
      if let details = presentation.executionDetails {
        ForEach(Array(details.enumerated()), id: \.offset) { _, detail in
          Text(detail).font(style.monoSmallFont).foregroundStyle(style.secondary)
            .textSelection(.enabled)
        }
      }
      ForEach(Array((presentation.toolArguments ?? []).enumerated()), id: \.offset) {
        _, argument in
        PermissionTextBox(label: argument.name, text: argument.value, style: style)
      }
      if presentation.fileChanges.isEmpty, let path = presentation.targetPath {
        Label(path, systemImage: "doc.text").font(style.monoSmallFont)
          .foregroundStyle(style.secondary).textSelection(.enabled)
      }
      ForEach(Array(presentation.fileChanges.enumerated()), id: \.offset) { _, change in
        PermissionFileChangeView(change: change, style: style)
      }
      if presentation.previewOmitted {
        Text("Diff preview unavailable; inspect the file before allowing.")
          .font(style.captionFont).foregroundStyle(style.warning)
      }
    }
  }
}

/// Exact text the person approves, such as a command or one tool argument.
private struct PermissionTextBox: View {
  let label: String
  let text: String
  let style: AgentConversationStyle

  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      Text(label).font(style.captionFont).foregroundStyle(style.secondary)
      PermissionWell(
        lines: PermissionPreviewPolicy.lineCount(text), axes: [.horizontal, .vertical],
        style: style
      ) {
        Text(text).foregroundStyle(style.ink)
          .textSelection(.enabled).fixedSize(horizontal: true, vertical: true)
          .frame(maxWidth: .infinity, alignment: .leading)
      }
    }
  }
}

/// A scrolling well whose first visible lines size it rigidly. A scroll view alone has no
/// minimum height, so a crowded pane collapsed it to an empty box; now the transcript yields.
private struct PermissionWell<Content: View>: View {
  let lines: Int
  let axes: Axis.Set
  let style: AgentConversationStyle
  @ViewBuilder let content: Content

  var body: some View {
    Text(PermissionPreviewPolicy.visibleLineSpacer(lines: lines))
      .hidden()
      .fixedSize(horizontal: false, vertical: true)
      .frame(maxWidth: .infinity, alignment: .leading)
      .overlay(alignment: .topLeading) { ScrollView(axes) { content } }
      .font(style.monoSmallFont)
      .padding(8)
      .background(style.well, in: RoundedRectangle(cornerRadius: 8))
  }
}

private struct PermissionFileChangeView: View {
  let change: PermissionFileChange
  let style: AgentConversationStyle

  private var preview: PermissionPreview { PermissionPreviewPolicy.preview(change) }

  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      Label(change.path, systemImage: "doc.text")
        .font(style.monoSmallFont).foregroundStyle(style.secondary)
        .textSelection(.enabled)
      if preview.unavailable {
        Text("Diff preview unavailable; inspect the file before allowing.")
          .font(style.captionFont).foregroundStyle(style.warning)
      } else {
        PermissionWell(lines: preview.rows.count, axes: .vertical, style: style) {
          LazyVStack(alignment: .leading, spacing: 0) {
            ForEach(preview.rows) { row in
              switch row.kind {
              case .removed:
                Text("− \(row.text)").foregroundStyle(style.danger)
              case .added:
                Text("+ \(row.text)").foregroundStyle(style.success)
              }
            }
          }.frame(maxWidth: .infinity, alignment: .leading)
            .textSelection(.enabled)
        }
      }
    }
  }
}
