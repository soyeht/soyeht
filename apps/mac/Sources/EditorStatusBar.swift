import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct EditorStatusBar: View {
  let view: EditorFileView
  let selectedPosition: TextPosition?
  let lsp: LspServerStatus?
  let diff: EditorDiffPresentation?
  let vimEnabled: Bool
  let vimMode: EditorVimMode
  let vimCommandLine: String?
  /// Opens "Trust this folder?" again; `nil` when the folder is not restricted.
  let onReviewTrust: (() -> Void)?

  var body: some View {
    HStack(spacing: MacTheme.Space.x14) {
      Text(language).font(MacTheme.ui(MacTheme.TypeScale.small)).help(languageHelp)
      Text("UTF-8").font(MacTheme.ui(MacTheme.TypeScale.small))
      Text(lineEnding).font(MacTheme.ui(MacTheme.TypeScale.small))
      if vimEnabled {
        Text(vimCommandLine ?? "-- \(vimMode.rawValue.uppercased()) --")
          .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
          .foregroundStyle(MacTheme.accent)
      }
      Text(
        "Ln \(identifier: UInt64(selectedPosition?.line ?? view.viewport?.fromLine ?? 0) + 1), Col \(identifier: UInt64(selectedPosition?.column ?? 0) + 1)"
      )
      .font(MacTheme.ui(MacTheme.TypeScale.small))
      Spacer(minLength: 0)
      if let blame = selectedBlame {
        Text("\(blame.author) · \(blame.summary)")
          .font(MacTheme.ui(MacTheme.TypeScale.small)).lineLimit(1)
          .help("\(blame.commit) · line \(UInt64(blame.line) + 1)")
      }
      if let onReviewTrust {
        Button(action: onReviewTrust) {
          Image(systemName: "lock.shield")
        }
        .help(ProjectTrustPresentation.restrictedNote)
        .accessibilityLabel(ProjectTrustPresentation.restrictedNote)
        .accessibilityIdentifier("editor-restricted-folder")
      }
      if !view.diagnostics.isEmpty {
        Circle().fill(MacTheme.warning).frame(width: 6, height: 6)
        Text("\(view.diagnostics.count) issues")
          .font(MacTheme.ui(MacTheme.TypeScale.small))
      }
      if let diff, !diff.markers.isEmpty {
        Circle().fill(MacTheme.diffAdded).frame(width: 6, height: 6)
        Text("\(diff.markers.count) changed lines")
          .font(MacTheme.ui(MacTheme.TypeScale.small))
      }
    }
    .foregroundStyle(MacTheme.secondary)
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: PaneLayoutContentMetrics.editorStatusHeight).background(MacTheme.canvas)
  }

  /// The line break of the line the caret is on, from the lines of the page.
  private var lineEnding: String {
    guard let viewport = view.viewport else { return "LF" }
    let caret = selectedPosition?.line ?? viewport.fromLine
    let line = Int(caret > viewport.fromLine ? caret - viewport.fromLine : 0)
    let ending = EditorEditing.lineEnding(lines: viewport.lines.map(\.text), line: line)
    return ending == "\r\n" ? "CRLF" : "LF"
  }

  private var language: String {
    view.file.language.rawValue.replacingOccurrences(of: "_", with: " ").capitalized
  }

  private var selectedBlame: FileBlame? {
    guard let line = selectedPosition?.line else { return nil }
    return diff?.blameByLine[line]
  }

  private var languageHelp: String {
    ProjectTrustPresentation.languageServerHelp(lsp)
  }
}
