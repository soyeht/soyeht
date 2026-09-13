import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct EditorPaneView: View {
  @Bindable var client: EngineClient
  let pane: Pane
  @State private var selectedPosition: TextPosition?
  @State private var showHistory = false
  @State private var showConflict = false
  @State private var references: [FileLocation] = []
  @State private var showReferences = false
  @State private var languageStatus: LspServerStatus?

  private var view: EditorFileView? {
    client.editorFiles.values.first { $0.file.paneId == pane.paneId }
  }

  var body: some View {
    VStack(spacing: MacTheme.Space.x8) {
      EditorPaneHeader(
        file: view?.file, branch: branch, showHistory: { showHistory = true },
        showConflict: { showConflict = true })
      if let view {
        if view.file.conflicted {
          EditorConflictBanner(showConflict: { showConflict = true })
        }
        EditorPaneBody(
          client: client, view: view, selectedPosition: $selectedPosition,
          languageStatus: languageStatus, definition: openDefinition, findReferences: findReferences
        )
      } else {
        editorLoading
      }
    }
    .padding(MacTheme.Space.x10)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .pane)
    .frame(minWidth: 620, maxWidth: .infinity, maxHeight: .infinity)
    .accessibilityIdentifier("editor-pane")
    .sheet(isPresented: $showHistory) {
      if let file = view?.file { EditorHistorySheet(client: client, file: file) }
    }
    .sheet(isPresented: $showConflict) {
      if let file = view?.file, file.conflicted {
        EditorConflictSheet(client: client, file: file)
      }
    }
    .sheet(isPresented: $showReferences) {
      EditorReferencesSheet(client: client, items: references, isPresented: $showReferences)
    }
    .task(id: languageStatusKey) { await pollLanguageStatus() }
  }

  private var branch: String? {
    client.tasks.first {
      $0.workspaceId == pane.workspaceId && $0.worktree?.path == pane.cwd
    }?.worktree?.branch
  }

  private var languageStatusKey: String {
    guard let file = view?.file else { return "none:\(client.connectionRevision)" }
    return "\(file.workspaceId):\(file.language.rawValue):\(client.connectionRevision)"
  }

  private func pollLanguageStatus() async {
    guard let file = view?.file else { return }
    while !Task.isCancelled {
      do {
        let result = try await client.languageServerStatus(
          workspaceId: file.workspaceId, language: file.language)
        languageStatus = result.servers.first { $0.language == file.language }
      } catch {
        languageStatus = nil
      }
      do { try await Task.sleep(for: .seconds(2)) } catch { return }
    }
  }

  private func openDefinition(_ position: TextPosition) {
    guard let file = view?.file else { return }
    Task { @MainActor in
      do {
        let result = try await client.editorDefinition(
          fileId: file.fileId, revision: file.revision, position: position)
        guard let location = result.location else {
          client.problem = "No definition was found at this position."
          return
        }
        _ = try await client.openEditorLocation(location)
      } catch { client.problem = error.localizedDescription }
    }
  }

  private func findReferences(_ position: TextPosition) {
    guard let file = view?.file else { return }
    Task { @MainActor in
      do {
        references = try await client.editorReferences(
          fileId: file.fileId, revision: file.revision, position: position
        ).items
        showReferences = true
      } catch { client.problem = error.localizedDescription }
    }
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
  let showHistory: () -> Void
  let showConflict: () -> Void

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
      Button(action: showHistory) {
        Image(systemName: "clock.arrow.circlepath")
          .font(.system(size: 11, weight: .medium))
      }
      .buttonStyle(.plain).foregroundStyle(MacTheme.secondary).help("File history")
      if file?.conflicted == true {
        Button(action: showConflict) {
          Text(statusLabel).font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
        }
        .buttonStyle(.plain).foregroundStyle(statusColor).help("Compare conflicting versions")
      } else {
        Text(statusLabel).font(MacTheme.chrome(MacTheme.TypeScale.small))
          .foregroundStyle(statusColor)
      }
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

private struct EditorConflictBanner: View {
  let showConflict: () -> Void

  var body: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Image(systemName: "exclamationmark.triangle.fill")
        .font(.system(size: 12, weight: .semibold)).foregroundStyle(MacTheme.danger)
      Text("The file changed on disk while this draft was open.")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .semibold))
        .foregroundStyle(MacTheme.ink)
      Spacer(minLength: 0)
      Button("Compare versions", action: showConflict)
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
        .buttonStyle(NeoPillButtonStyle())
    }
    .padding(.horizontal, MacTheme.Space.x10).frame(height: 34)
    .background(MacTheme.warning.opacity(0.12), in: RoundedRectangle(cornerRadius: 9))
  }
}

private struct EditorPaneBody: View {
  @Bindable var client: EngineClient
  let view: EditorFileView
  @Binding var selectedPosition: TextPosition?
  let languageStatus: LspServerStatus?
  let definition: (TextPosition) -> Void
  let findReferences: (TextPosition) -> Void

  var body: some View {
    HStack(spacing: 0) {
      EditorFileTree(client: client, selected: view.file)
      VStack(spacing: 0) {
        EditorTabs(client: client, selected: view.file)
        EditorCodeViewport(
          client: client, view: view, selectedPosition: $selectedPosition,
          definition: definition, findReferences: findReferences)
        EditorStatusBar(view: view, selectedPosition: selectedPosition, lsp: languageStatus)
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
  @Bindable var client: EngineClient
  let view: EditorFileView
  @Binding var selectedPosition: TextPosition?
  let definition: (TextPosition) -> Void
  let findReferences: (TextPosition) -> Void

  var body: some View {
    EditorNativeViewport(
      client: client, view: view, selectedPosition: $selectedPosition,
      onDefinition: definition, onReferences: findReferences
    )
    .background(MacTheme.face)
  }
}

private struct EditorStatusBar: View {
  let view: EditorFileView
  let selectedPosition: TextPosition?
  let lsp: LspServerStatus?

  var body: some View {
    HStack(spacing: MacTheme.Space.x14) {
      Text(language).font(MacTheme.chrome(MacTheme.TypeScale.small)).help(languageHelp)
      Text("UTF-8").font(MacTheme.chrome(MacTheme.TypeScale.small))
      Text("LF").font(MacTheme.chrome(MacTheme.TypeScale.small))
      Text(
        "Ln \(UInt64(selectedPosition?.line ?? view.viewport?.fromLine ?? 0) + 1), Col \(UInt64(selectedPosition?.column ?? 0) + 1)"
      )
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

  private var languageHelp: String {
    guard let lsp else { return "No language server is active" }
    return lsp.detail.map { "Language server: \(lsp.state.rawValue) · \($0)" }
      ?? "Language server: \(lsp.state.rawValue)"
  }
}

private struct EditorReferencesSheet: View {
  @Bindable var client: EngineClient
  let items: [FileLocation]
  @Binding var isPresented: Bool

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      HStack {
        Text("References")
          .font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
        Spacer()
        Button("Done") { isPresented = false }.buttonStyle(NeoPillButtonStyle())
      }
      if items.isEmpty {
        Text("No references found")
          .font(MacTheme.chrome(MacTheme.TypeScale.caption)).foregroundStyle(MacTheme.secondary)
          .frame(maxWidth: .infinity, maxHeight: .infinity)
      } else {
        ScrollView {
          LazyVStack(spacing: MacTheme.Space.x6) {
            ForEach(Array(items.enumerated()), id: \.offset) { _, item in
              Button {
                open(item)
              } label: {
                HStack {
                  Text(item.path).font(MacTheme.mono(MacTheme.TypeScale.caption))
                  Spacer()
                  Text(
                    "\(UInt64(item.range.start.line) + 1):\(UInt64(item.range.start.column) + 1)"
                  )
                  .font(MacTheme.mono(MacTheme.TypeScale.small))
                  .foregroundStyle(MacTheme.secondary)
                }
                .padding(.horizontal, MacTheme.Space.x10).frame(height: 38)
                .background(MacTheme.face, in: RoundedRectangle(cornerRadius: 8))
              }
            }
          }
        }
      }
    }
    .padding(MacTheme.Space.x16).frame(width: 560, height: 420)
    .background(MacTheme.canvas).foregroundStyle(MacTheme.ink)
  }

  private func open(_ location: FileLocation) {
    Task { @MainActor in
      do {
        _ = try await client.openEditorLocation(location)
        isPresented = false
      } catch { client.problem = error.localizedDescription }
    }
  }
}
