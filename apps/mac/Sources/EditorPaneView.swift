import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct EditorPaneView: View {
  @Bindable var client: EngineClient
  let pane: Pane
  @State private var selectedLine: UInt32?

  private var view: EditorFileView? {
    client.editorFiles.values.first { $0.file.paneId == pane.paneId }
  }

  var body: some View {
    VStack(spacing: MacTheme.Space.x8) {
      EditorPaneHeader(file: view?.file, branch: branch)
      if let view {
        EditorPaneBody(client: client, view: view, selectedLine: $selectedLine)
      } else {
        editorLoading
      }
    }
    .padding(MacTheme.Space.x10)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
    .frame(minWidth: 620, maxWidth: .infinity, maxHeight: .infinity)
    .accessibilityIdentifier("editor-pane")
  }

  private var branch: String? {
    client.tasks.first {
      $0.workspaceId == pane.workspaceId && $0.worktree?.path == pane.cwd
    }?.worktree?.branch
  }

  private var editorLoading: some View {
    VStack(spacing: MacTheme.Space.x10) {
      ProgressView()
      Text("Loading editor viewport")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
        .foregroundStyle(MacTheme.secondary)
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
  }
}

private struct EditorPaneHeader: View {
  let file: SoyehtProtocol.FileDocument?
  let branch: String?

  var body: some View {
    HStack(spacing: MacTheme.Space.x7) {
      Image(systemName: "doc.text")
        .font(.system(size: 12, weight: .medium))
        .foregroundStyle(MacTheme.ink)
      Text(pathLabel)
        .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
        .foregroundStyle(MacTheme.ink).lineLimit(1).truncationMode(.head)
      Spacer(minLength: MacTheme.Space.x8)
      if let branch {
        Image(systemName: "arrow.triangle.branch")
          .font(.system(size: 11, weight: .medium)).foregroundStyle(MacTheme.muted)
        Text(branch).font(MacTheme.mono(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.muted)
      }
      Text(statusLabel).font(MacTheme.chrome(MacTheme.TypeScale.small))
        .foregroundStyle(statusColor)
    }
    .padding(.horizontal, MacTheme.Space.x12).frame(height: 27)
    .background(MacTheme.well, in: Capsule())
  }

  private var pathLabel: String {
    guard let file else { return "Editor" }
    return file.path.split(separator: "/").joined(separator: " › ")
  }

  private var statusLabel: String {
    guard let file else { return "· loading" }
    if file.conflicted { return "· conflict" }
    return file.dirty ? "· unsaved" : "· saved"
  }

  private var statusColor: Color {
    if file?.conflicted == true { return MacTheme.danger }
    if file?.dirty == true { return MacTheme.warning }
    return MacTheme.muted
  }
}

private struct EditorPaneBody: View {
  @Bindable var client: EngineClient
  let view: EditorFileView
  @Binding var selectedLine: UInt32?

  var body: some View {
    HStack(spacing: 0) {
      EditorFileTree(client: client, selected: view.file)
      VStack(spacing: 0) {
        EditorTabs(client: client, selected: view.file)
        EditorCodeViewport(view: view, selectedLine: $selectedLine)
        EditorStatusBar(view: view, selectedLine: selectedLine)
      }
    }
    .background(MacTheme.face)
    .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control))
  }
}

private struct EditorFileTree: View {
  @Bindable var client: EngineClient
  let selected: SoyehtProtocol.FileDocument

  var body: some View {
    VStack(spacing: MacTheme.Space.x2) {
      HStack(spacing: MacTheme.Space.x6) {
        Text("FILES").font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
          .tracking(0.8).foregroundStyle(MacTheme.secondary)
        Spacer()
        Image(systemName: "magnifyingglass")
          .font(.system(size: 12, weight: .medium)).foregroundStyle(MacTheme.muted)
      }
      .padding(.horizontal, MacTheme.Space.x6).frame(height: 22)
      ForEach(treeRows) { row in
        HStack(spacing: MacTheme.Space.x6) {
          Image(systemName: row.folder ? "folder" : "doc.text")
            .font(.system(size: 11, weight: .medium))
            .foregroundStyle(row.selected ? MacTheme.accent : MacTheme.secondary)
          Text(row.name)
            .font(MacTheme.mono(11.5, weight: row.selected ? .bold : .regular))
            .foregroundStyle(row.selected ? MacTheme.accent : MacTheme.ink)
            .lineLimit(1)
          Spacer(minLength: 0)
        }
        .padding(.leading, CGFloat(row.depth) * 12)
        .padding(.horizontal, MacTheme.Space.x8).frame(height: 22)
        .background(
          row.selected ? MacTheme.well : Color.clear, in: RoundedRectangle(cornerRadius: 6)
        )
        .contentShape(Rectangle())
        .onTapGesture { if let paneId = row.paneId { client.select(paneId) } }
      }
      Spacer(minLength: 0)
    }
    .padding(.vertical, MacTheme.Space.x10).padding(.horizontal, MacTheme.Space.x8)
    .frame(width: 212).background(MacTheme.canvas)
  }

  private var treeRows: [EditorTreeRow] {
    let files = client.editorFiles.values.map(\.file)
      .filter { $0.workspaceId == selected.workspaceId }.sorted { $0.path < $1.path }
    let workspace = client.workspaces.first { $0.workspaceId == selected.workspaceId }
    var rows = [
      EditorTreeRow(id: "root", name: workspace?.name ?? "Workspace", depth: 0, folder: true)
    ]
    var seen: Set<String> = []
    for file in files {
      let parts = file.path.split(separator: "/").map(String.init)
      for index in parts.indices {
        let key = parts[...index].joined(separator: "/")
        guard seen.insert(key).inserted else { continue }
        let isFile = index == parts.index(before: parts.endIndex)
        rows.append(
          EditorTreeRow(
            id: key, name: parts[index], depth: index + 1, folder: !isFile,
            paneId: isFile ? file.paneId : nil, selected: isFile && file.fileId == selected.fileId))
      }
    }
    return rows
  }
}

private struct EditorTreeRow: Identifiable {
  let id: String
  let name: String
  let depth: Int
  let folder: Bool
  var paneId: String?
  var selected = false
}

private struct EditorTabs: View {
  @Bindable var client: EngineClient
  let selected: SoyehtProtocol.FileDocument

  var body: some View {
    HStack(spacing: MacTheme.Space.x6) {
      ForEach(files, id: \.fileId) { file in
        Button {
          client.select(file.paneId)
        } label: {
          HStack(spacing: MacTheme.Space.x6) {
            Image(systemName: "doc.text")
              .font(.system(size: 11, weight: .medium))
            Text(fileName(file.path)).font(MacTheme.mono(11.5, weight: tabWeight(file)))
            if file.dirty {
              Circle().fill(MacTheme.warning).frame(width: 6, height: 6)
            } else {
              Image(systemName: "xmark")
                .font(.system(size: 9, weight: .medium))
                .accessibilityHidden(true)
            }
          }
          .foregroundStyle(file.fileId == selected.fileId ? MacTheme.accent : MacTheme.secondary)
          .padding(.horizontal, MacTheme.Space.x12).frame(height: 26)
          .background(
            file.fileId == selected.fileId ? MacTheme.well : Color.clear,
            in: Capsule())
        }
      }
      Spacer(minLength: 0)
      if let cursor = agentCursor {
        HStack(spacing: MacTheme.Space.x6) {
          Circle().fill(MacTheme.ink).frame(width: 6, height: 6)
          Text("\(cursor.name) · editing")
            .font(MacTheme.mono(10.5, weight: .bold)).foregroundStyle(MacTheme.ink)
        }
        .padding(.horizontal, MacTheme.Space.x10).frame(height: 22)
        .background(MacTheme.agentBlue, in: Capsule())
      }
    }
    .padding(.horizontal, MacTheme.Space.x8).frame(height: 34).background(MacTheme.canvas)
  }

  private var files: [SoyehtProtocol.FileDocument] {
    client.editorFiles.values.map(\.file)
      .filter { $0.workspaceId == selected.workspaceId }.sorted { $0.path < $1.path }
  }
  private var agentCursor: EditorAgentCursorEvent? {
    client.editorFiles[selected.fileId]?.agentCursors.values.sorted { $0.name < $1.name }.first
  }
  private func fileName(_ path: String) -> String {
    String(path.split(separator: "/").last ?? "File")
  }
  private func tabWeight(_ file: SoyehtProtocol.FileDocument) -> Font.Weight {
    file.fileId == selected.fileId ? .bold : .regular
  }
}

private struct EditorCodeViewport: View {
  let view: EditorFileView
  @Binding var selectedLine: UInt32?

  var body: some View {
    GeometryReader { geometry in
      ScrollView([.horizontal, .vertical]) {
        LazyVStack(alignment: .leading, spacing: 0) {
          ForEach(view.viewport?.lines ?? [], id: \.line) { line in
            EditorCodeLine(
              line: line, changed: view.changedLines.contains(line.line),
              selected: selectedLine == line.line,
              hasAgentCursor: view.agentCursors.values.contains { $0.position.line == line.line }
            )
            .onTapGesture { selectedLine = line.line }
          }
        }
        .frame(minWidth: geometry.size.width, alignment: .leading)
        .padding(.top, MacTheme.Space.x10)
      }
      .background(MacTheme.face)
    }
  }
}

private struct EditorCodeLine: View {
  let line: EditorLine
  let changed: Bool
  let selected: Bool
  let hasAgentCursor: Bool

  var body: some View {
    HStack(spacing: 0) {
      RoundedRectangle(cornerRadius: 2).fill(changed ? MacTheme.diffAdded : MacTheme.face)
        .frame(width: 3, height: 19)
      Text("\(UInt64(line.line) + 1)")
        .font(MacTheme.mono(MacTheme.TypeScale.small))
        .foregroundStyle(selected ? MacTheme.ink : MacTheme.muted)
        .frame(width: 41, alignment: .trailing).padding(.trailing, MacTheme.Space.x10)
      EditorHighlightedText(line: line)
        .frame(minWidth: 760, alignment: .leading).padding(.horizontal, MacTheme.Space.x10)
    }
    .frame(height: 19)
    .background(selected || hasAgentCursor ? MacTheme.well : Color.clear)
  }
}

private struct EditorHighlightedText: View {
  let line: EditorLine

  var body: some View {
    rendered.font(MacTheme.mono(11.5)).lineLimit(1).fixedSize(horizontal: true, vertical: false)
  }

  private var rendered: Text {
    let source = line.text as NSString
    let count = source.length
    var cursor = 0
    var result = Text("")
    for span in line.highlights.sorted(by: { $0.startColumn < $1.startColumn }) {
      let start = min(Int(span.startColumn), count)
      let end = min(max(Int(span.endColumn), start), count)
      guard start >= cursor else { continue }
      if start > cursor {
        result =
          result
          + Text(source.substring(with: NSRange(location: cursor, length: start - cursor)))
          .foregroundColor(MacTheme.ink)
      }
      if end > start {
        result =
          result
          + Text(source.substring(with: NSRange(location: start, length: end - start)))
          .foregroundColor(color(span.kind))
      }
      cursor = end
    }
    if cursor < count {
      result = result + Text(source.substring(from: cursor)).foregroundColor(MacTheme.ink)
    }
    return result
  }

  private func color(_ kind: HighlightKind) -> Color {
    switch kind {
    case .keyword, .function, .operator: MacTheme.accent
    case .type: MacTheme.syntaxType
    case .number, .constant: MacTheme.warning
    case .comment: MacTheme.muted
    case .string: MacTheme.success
    case .property, .punctuation, .variable: MacTheme.ink
    }
  }
}

private struct EditorStatusBar: View {
  let view: EditorFileView
  let selectedLine: UInt32?

  var body: some View {
    HStack(spacing: MacTheme.Space.x14) {
      Text(language).font(MacTheme.chrome(MacTheme.TypeScale.small))
      Text("UTF-8").font(MacTheme.chrome(MacTheme.TypeScale.small))
      Text("LF").font(MacTheme.chrome(MacTheme.TypeScale.small))
      Text("Ln \(UInt64(selectedLine ?? view.viewport?.fromLine ?? 0) + 1), Col 1")
        .font(MacTheme.chrome(MacTheme.TypeScale.small))
      Spacer(minLength: 0)
      if !view.diagnostics.isEmpty {
        Circle().fill(MacTheme.warning).frame(width: 6, height: 6)
        Text("\(view.diagnostics.count) issues")
          .font(MacTheme.chrome(MacTheme.TypeScale.small))
      }
      if !view.changedLines.isEmpty {
        Circle().fill(MacTheme.diffAdded).frame(width: 6, height: 6)
        Text("+\(view.changedLines.count) changed lines")
          .font(MacTheme.chrome(MacTheme.TypeScale.small))
      }
    }
    .foregroundStyle(MacTheme.secondary)
    .padding(.horizontal, MacTheme.Space.x12).frame(height: 24).background(MacTheme.canvas)
  }

  private var language: String {
    view.file.language.rawValue.replacingOccurrences(of: "_", with: " ").capitalized
  }
}
