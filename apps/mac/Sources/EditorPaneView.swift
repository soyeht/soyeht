import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct EditorPaneView: View {
  @Bindable var client: EngineClient
  @Environment(EditorInputPreferences.self) private var inputPreferences
  let pane: Pane
  @State private var selectedPosition: TextPosition?
  @State private var showHistory = false
  @State private var showConflict = false
  @State private var references: [FileLocation] = []
  @State private var showReferences = false
  @State private var languageStatus: LspServerStatus?
  @State private var diff: FileDiffResult?
  @State private var diffProblem: String?
  @State private var showDiff = false
  @State private var vimMode = EditorVimMode.normal
  @State private var vimCommandLine: String?

  private var view: EditorFileView? {
    client.editorFiles.values.first { $0.file.paneId == pane.paneId }
  }

  private var currentDiff: FileDiffResult? {
    guard let file = view?.file, let diff,
      file.fileId == diff.fileId, file.revision == diff.revision
    else { return nil }
    return diff
  }

  var body: some View {
    VStack(spacing: MacTheme.Space.x8) {
      EditorPaneHeader(
        file: view?.file, branch: branch, showHistory: { showHistory = true },
        showConflict: { showConflict = true }, showDiff: { showDiff = true },
        diffReady: currentDiff != nil, diffProblem: diffProblem)
      if let view {
        if view.file.conflicted {
          EditorConflictBanner(showConflict: { showConflict = true })
        }
        EditorPaneBody(
          client: client, view: view, selectedPosition: $selectedPosition,
          navigationTarget: client.editorNavigationTarget,
          languageStatus: languageStatus, diff: currentDiff.map(EditorDiffPresentation.init),
          vimEnabled: inputPreferences.vimEnabled, vimMode: $vimMode,
          vimCommandLine: $vimCommandLine,
          definition: openDefinition, findReferences: findReferences
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
    .sheet(isPresented: $showDiff) {
      if let file = view?.file, let diff = currentDiff {
        EditorDiffSheet(file: file, result: diff, isPresented: $showDiff)
      }
    }
    .task(id: languageStatusKey) { await pollLanguageStatus() }
    .task(id: diffKey) { await loadDiff() }
    .task(id: viewportLoadKey) { await loadViewportIfNeeded() }
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

  private var diffKey: String {
    guard let file = view?.file else { return "none:\(client.connectionRevision)" }
    return "\(file.fileId):\(file.revision):\(client.connectionRevision)"
  }

  private var viewportLoadKey: String {
    guard let view else { return "none:\(client.connectionRevision)" }
    return
      "\(view.file.fileId):\(view.file.revision):\(view.viewport == nil):\(client.connectionRevision)"
  }

  private func loadViewportIfNeeded() async {
    guard let view, view.viewport == nil else { return }
    do {
      try await client.loadEditorViewport(fileId: view.file.fileId, fromLine: 0)
    } catch is CancellationError {
      return
    } catch {
      client.problem = error.localizedDescription
    }
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

  private func loadDiff() async {
    diff = nil
    diffProblem = nil
    guard let file = view?.file else { return }
    do {
      try await Task.sleep(for: .milliseconds(120))
      let result = try await client.editorDiff(fileId: file.fileId, revision: file.revision)
      guard !Task.isCancelled,
        view?.file.fileId == result.fileId,
        view?.file.revision == result.revision
      else { return }
      diff = result
    } catch is CancellationError {
      return
    } catch {
      guard !Task.isCancelled else { return }
      diffProblem = error.localizedDescription
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
  let showDiff: () -> Void
  let diffReady: Bool
  let diffProblem: String?

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
      Button(action: showDiff) {
        Image(systemName: "rectangle.split.2x1")
          .font(.system(size: 11, weight: .medium))
      }
      .buttonStyle(.plain)
      .foregroundStyle(diffProblem == nil ? MacTheme.secondary : MacTheme.danger)
      .disabled(!diffReady)
      .help(diffProblem ?? "Compare the current buffer with HEAD")
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
  let navigationTarget: EditorNavigationTarget?
  let languageStatus: LspServerStatus?
  let diff: EditorDiffPresentation?
  let vimEnabled: Bool
  @Binding var vimMode: EditorVimMode
  @Binding var vimCommandLine: String?
  let definition: (TextPosition) -> Void
  let findReferences: (TextPosition) -> Void

  var body: some View {
    HStack(spacing: 0) {
      EditorFileTree(client: client, selected: view.file)
      VStack(spacing: 0) {
        EditorTabs(client: client, selected: view.file)
        EditorCodeViewport(
          client: client, view: view, selectedPosition: $selectedPosition,
          navigationTarget: navigationTarget,
          diff: diff, vimEnabled: vimEnabled, vimMode: $vimMode,
          vimCommandLine: $vimCommandLine, definition: definition,
          findReferences: findReferences)
        if view.review != nil {
          EditorAgentReviewBar(
            client: client, view: view, selectedPosition: selectedPosition)
        }
        EditorStatusBar(
          view: view, selectedPosition: selectedPosition, lsp: languageStatus, diff: diff,
          vimEnabled: vimEnabled, vimMode: vimMode, vimCommandLine: vimCommandLine)
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
          Text(
            "\(cursor.name) · \(UInt64(cursor.position.line) + 1):\(UInt64(cursor.position.column) + 1)"
          )
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
  let navigationTarget: EditorNavigationTarget?
  let diff: EditorDiffPresentation?
  let vimEnabled: Bool
  @Binding var vimMode: EditorVimMode
  @Binding var vimCommandLine: String?
  let definition: (TextPosition) -> Void
  let findReferences: (TextPosition) -> Void

  var body: some View {
    HStack(spacing: 0) {
      EditorNativeViewport(
        client: client, view: view, selectedPosition: $selectedPosition,
        navigationTarget: navigationTarget,
        diffMarkers: diff?.gutterMarkers(lineCount: view.file.lineCount) ?? [:],
        agentReviewLines: reviewPresentation?.lines ?? [],
        agentCursors: Array(view.agentCursors.values),
        vimEnabled: vimEnabled, vimMode: $vimMode, vimCommandLine: $vimCommandLine,
        onDefinition: definition, onReferences: findReferences
      )
      .background(MacTheme.face)
      if view.file.language == .markdown, let viewport = view.viewport {
        Rectangle().fill(MacTheme.well).frame(width: 1)
        EditorMarkdownPreview(viewport: viewport)
      }
    }
  }

  private var reviewPresentation: EditorAgentReviewPresentation? {
    view.review.map {
      EditorAgentReviewPresentation(
        $0, fallbackName: view.agentCursors[$0.paneId]?.name ?? "Agent")
    }
  }
}

private struct EditorAgentReviewBar: View {
  @Bindable var client: EngineClient
  let view: EditorFileView
  let selectedPosition: TextPosition?
  @State private var resolvingHunkId: String?
  @State private var showComment = false
  @State private var commentBody = ""

  private var presentation: EditorAgentReviewPresentation? {
    view.review.map {
      EditorAgentReviewPresentation(
        $0, fallbackName: view.agentCursors[$0.paneId]?.name ?? "Agent")
    }
  }

  var body: some View {
    if let presentation {
      VStack(alignment: .leading, spacing: MacTheme.Space.x6) {
        HStack(spacing: MacTheme.Space.x8) {
          Image(systemName: "person.crop.circle.badge.checkmark")
            .font(.system(size: 12, weight: .semibold)).foregroundStyle(MacTheme.accent)
          Text(
            "Review \(presentation.hunks.count) agent \(presentation.hunks.count == 1 ? "change" : "changes")"
          )
          .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
          Spacer(minLength: 0)
          Button(commentLabel) { showComment = true }
            .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
            .buttonStyle(NeoPillButtonStyle())
            .disabled(selectedPosition == nil)
        }
        ScrollView(.horizontal) {
          HStack(spacing: MacTheme.Space.x8) {
            ForEach(presentation.hunks) { hunk in
              EditorAgentReviewHunkCard(
                hunk: hunk, busy: resolvingHunkId != nil,
                accept: { resolve(hunk, accept: true) },
                reject: { resolve(hunk, accept: false) })
            }
          }
        }
        .scrollIndicators(.hidden)
      }
      .padding(.horizontal, MacTheme.Space.x10).padding(.vertical, MacTheme.Space.x8)
      .background(MacTheme.canvas)
      .sheet(isPresented: $showComment) {
        EditorReviewCommentSheet(
          line: selectedPosition?.line, comment: $commentBody, isPresented: $showComment,
          submit: submitComment)
      }
    }
  }

  private var commentLabel: String {
    selectedPosition.map { "Comment on line \(UInt64($0.line) + 1)" } ?? "Select a line"
  }

  private func resolve(_ hunk: EditorAgentReviewHunk, accept: Bool) {
    guard let review = view.review else { return }
    resolvingHunkId = hunk.id
    Task { @MainActor in
      defer { resolvingHunkId = nil }
      do {
        if accept {
          _ = try await client.acceptEditorReview(
            fileId: view.file.fileId, hunkId: hunk.id, baseRevision: review.revision)
        } else {
          _ = try await client.rejectEditorReview(
            fileId: view.file.fileId, hunkId: hunk.id, baseRevision: review.revision)
        }
      } catch { client.problem = error.localizedDescription }
    }
  }

  private func submitComment() {
    guard let position = selectedPosition else { return }
    let body = commentBody.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !body.isEmpty else { return }
    Task { @MainActor in
      do {
        let range = SoyehtProtocol.TextRange(start: position, end: position)
        let result = try await client.commentOnEditorReview(
          fileId: view.file.fileId, range: range, body: body)
        commentBody = ""
        showComment = false
        if !result.delivered { client.problem = "The author pane is no longer available." }
      } catch { client.problem = error.localizedDescription }
    }
  }
}

private struct EditorAgentReviewHunkCard: View {
  let hunk: EditorAgentReviewHunk
  let busy: Bool
  let accept: () -> Void
  let reject: () -> Void

  var body: some View {
    HStack(spacing: MacTheme.Space.x8) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
        Text("\(hunk.authorName) · line \(UInt64(hunk.line) + 1)")
          .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
          .foregroundStyle(MacTheme.ink)
        Text(hunk.summary.isEmpty ? "Agent change" : hunk.summary)
          .font(MacTheme.mono(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.secondary)
          .lineLimit(1).frame(maxWidth: 210, alignment: .leading)
      }
      Text("\(hunk.changedLineCount)")
        .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
        .foregroundStyle(MacTheme.accent)
      Button("Accept", action: accept)
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
        .buttonStyle(NeoPillButtonStyle(accent: true)).disabled(busy)
      Button("Reject", action: reject)
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
        .buttonStyle(NeoPillButtonStyle()).disabled(busy)
    }
    .padding(.horizontal, MacTheme.Space.x10).frame(height: 42)
    .background(MacTheme.agentBlue, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
  }
}

private struct EditorReviewCommentSheet: View {
  let line: UInt32?
  @Binding var comment: String
  @Binding var isPresented: Bool
  let submit: () -> Void

  private var contentReady: Bool {
    !comment.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
  }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      Text("Request a change on line \(UInt64(line ?? 0) + 1)")
        .font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
      Text("The request goes to the agent that authored the pending change.")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption)).foregroundStyle(MacTheme.secondary)
      TextEditor(text: $comment)
        .font(MacTheme.chrome(MacTheme.TypeScale.caption)).scrollContentBackground(.hidden)
        .padding(MacTheme.Space.x8).frame(minHeight: 120)
        .neoWell(radius: MacTheme.Radius.control)
      HStack {
        Spacer()
        Button("Cancel") { isPresented = false }.buttonStyle(NeoPillButtonStyle())
        Button("Send request", action: submit)
          .buttonStyle(NeoPillButtonStyle(accent: true)).disabled(!contentReady)
      }
    }
    .padding(MacTheme.Space.x16).frame(width: 480)
    .background(MacTheme.canvas).foregroundStyle(MacTheme.ink)
  }
}

private struct EditorStatusBar: View {
  let view: EditorFileView
  let selectedPosition: TextPosition?
  let lsp: LspServerStatus?
  let diff: EditorDiffPresentation?
  let vimEnabled: Bool
  let vimMode: EditorVimMode
  let vimCommandLine: String?

  var body: some View {
    HStack(spacing: MacTheme.Space.x14) {
      Text(language).font(MacTheme.chrome(MacTheme.TypeScale.small)).help(languageHelp)
      Text("UTF-8").font(MacTheme.chrome(MacTheme.TypeScale.small))
      Text("LF").font(MacTheme.chrome(MacTheme.TypeScale.small))
      if vimEnabled {
        Text(vimCommandLine ?? "-- \(vimMode.rawValue.uppercased()) --")
          .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
          .foregroundStyle(MacTheme.accent)
      }
      Text(
        "Ln \(UInt64(selectedPosition?.line ?? view.viewport?.fromLine ?? 0) + 1), Col \(UInt64(selectedPosition?.column ?? 0) + 1)"
      )
      .font(MacTheme.chrome(MacTheme.TypeScale.small))
      Spacer(minLength: 0)
      if let blame = selectedBlame {
        Text("\(blame.author) · \(blame.summary)")
          .font(MacTheme.chrome(MacTheme.TypeScale.small)).lineLimit(1)
          .help("\(blame.commit) · line \(UInt64(blame.line) + 1)")
      }
      if !view.diagnostics.isEmpty {
        Circle().fill(MacTheme.warning).frame(width: 6, height: 6)
        Text("\(view.diagnostics.count) issues")
          .font(MacTheme.chrome(MacTheme.TypeScale.small))
      }
      if let diff, !diff.markers.isEmpty {
        Circle().fill(MacTheme.diffAdded).frame(width: 6, height: 6)
        Text("\(diff.markers.count) changed lines")
          .font(MacTheme.chrome(MacTheme.TypeScale.small))
      }
    }
    .foregroundStyle(MacTheme.secondary)
    .padding(.horizontal, MacTheme.Space.x12).frame(height: 24).background(MacTheme.canvas)
  }

  private var language: String {
    view.file.language.rawValue.replacingOccurrences(of: "_", with: " ").capitalized
  }

  private var selectedBlame: FileBlame? {
    guard let line = selectedPosition?.line else { return nil }
    return diff?.blameByLine[line]
  }

  private var languageHelp: String {
    guard let lsp else { return "No language server is active" }
    return lsp.detail.map { "Language server: \(lsp.state.rawValue) · \($0)" }
      ?? "Language server: \(lsp.state.rawValue)"
  }
}

private struct EditorDiffSheet: View {
  let file: SoyehtProtocol.FileDocument
  let result: FileDiffResult
  @Binding var isPresented: Bool

  private var presentation: EditorDiffPresentation { EditorDiffPresentation(result) }

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x12) {
      header
      columnLabels
      if presentation.rows.isEmpty {
        emptyState
      } else {
        diffRows
      }
    }
    .padding(MacTheme.Space.x14).frame(width: 980, height: 620)
    .background(MacTheme.canvas).foregroundStyle(MacTheme.ink)
  }

  private var header: some View {
    HStack(spacing: MacTheme.Space.x10) {
      VStack(alignment: .leading, spacing: MacTheme.Space.x3) {
        Text("Changes against HEAD")
          .font(MacTheme.chrome(MacTheme.TypeScale.title, weight: .bold))
        Text(file.path)
          .font(MacTheme.mono(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.secondary)
      }
      Spacer()
      Text(changeSummary)
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .bold))
        .foregroundStyle(MacTheme.secondary)
      Button("Done") { isPresented = false }.buttonStyle(NeoPillButtonStyle())
    }
  }

  private var columnLabels: some View {
    HStack(spacing: 1) {
      diffColumnLabel("HEAD")
      diffColumnLabel("CURRENT BUFFER")
    }
  }

  private var diffRows: some View {
    ScrollView([.horizontal, .vertical]) {
      LazyVStack(spacing: 1) {
        ForEach(presentation.rows) { row in
          HStack(spacing: 1) {
            EditorDiffCell(side: row.old, kind: row.kind, current: false, blame: nil)
            EditorDiffCell(
              side: row.new, kind: row.kind, current: true,
              blame: row.new.line.flatMap { presentation.blameByLine[$0] })
          }
        }
      }
    }
    .neoWell(radius: MacTheme.Radius.control)
  }

  private var emptyState: some View {
    VStack(spacing: MacTheme.Space.x8) {
      Image(systemName: "checkmark.circle")
        .font(.system(size: 24, weight: .medium)).foregroundStyle(MacTheme.success)
      Text("The current buffer matches HEAD")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
        .foregroundStyle(MacTheme.secondary)
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
    .neoWell(radius: MacTheme.Radius.control)
  }

  private func diffColumnLabel(_ label: String) -> some View {
    Text(label).font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
      .tracking(0.8).foregroundStyle(MacTheme.secondary)
      .frame(maxWidth: .infinity, alignment: .leading)
      .padding(.horizontal, MacTheme.Space.x10).frame(height: 22)
  }

  private var changeSummary: String {
    let changed = presentation.rows.filter { $0.kind != .context }.count
    return "\(changed) changed \(changed == 1 ? "line" : "lines")"
  }
}

private struct EditorDiffCell: View {
  let side: EditorDiffSide
  let kind: EditorDiffRowKind
  let current: Bool
  let blame: FileBlame?

  var body: some View {
    HStack(spacing: 0) {
      Text(side.line.map { "\(UInt64($0) + 1)" } ?? "")
        .font(MacTheme.mono(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.muted)
        .frame(width: 38, alignment: .trailing).padding(.trailing, MacTheme.Space.x6)
      Text(prefix)
        .font(MacTheme.mono(MacTheme.TypeScale.small, weight: .bold))
        .foregroundStyle(markerColor).frame(width: 14)
      Text(side.text ?? "")
        .font(MacTheme.mono(11.5)).foregroundStyle(MacTheme.ink)
        .frame(minWidth: 390, maxWidth: .infinity, alignment: .leading)
      if let author = side.author {
        Text(author.name).font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
          .foregroundStyle(MacTheme.secondary).padding(.horizontal, MacTheme.Space.x6)
      }
    }
    .frame(height: 20).background(cellBackground)
    .help(blameHelp)
  }

  private var prefix: String {
    switch (kind, current) {
    case (.added, true): "+"
    case (.removed, false): "−"
    case (.modified, true): "+"
    case (.modified, false): "−"
    default: " "
    }
  }

  private var markerColor: Color {
    switch kind {
    case .added: MacTheme.diffAdded
    case .removed: MacTheme.danger
    case .modified: MacTheme.warning
    case .context: MacTheme.muted
    }
  }

  private var cellBackground: Color {
    guard side.text != nil else { return MacTheme.well.opacity(0.45) }
    switch kind {
    case .added: return current ? MacTheme.diffAdded.opacity(0.12) : MacTheme.face
    case .removed: return current ? MacTheme.face : MacTheme.danger.opacity(0.10)
    case .modified: return MacTheme.warning.opacity(0.10)
    case .context: return MacTheme.face
    }
  }

  private var blameHelp: String {
    guard let blame else { return "" }
    return "\(blame.author) · \(blame.summary) · \(blame.commit)"
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
