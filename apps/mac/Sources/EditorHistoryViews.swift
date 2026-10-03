import Foundation
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct EditorHistorySheet: View {
  @Bindable var client: EngineClient
  let file: SoyehtProtocol.FileDocument
  @Environment(\.dismiss) private var dismiss
  @State private var versions: [FileVersion] = []
  @State private var nextCursor: String?
  @State private var selected: FileVersion?
  @State private var preview: FileReadResult?
  @State private var loading = true
  @State private var problem: String?

  var body: some View {
    VStack(spacing: MacTheme.Space.x12) {
      title
      HStack(spacing: MacTheme.Space.x12) {
        versionList
        Divider().overlay(MacTheme.well)
        EditorVersionPreview(
          title: previewTitle, viewport: preview,
          placeholder: selected == nil ? "Choose a saved version" : "Loading version")
      }
      footer
    }
    .padding(MacTheme.Space.x14).macSheetSurface(.editorHistory).background(MacTheme.canvas)
    .task(id: file.fileId) { await loadFirstPage() }
  }

  private var title: some View {
    HStack {
      VStack(alignment: .leading, spacing: MacTheme.Space.x3) {
        Text("Local history")
          .font(MacTheme.ui(MacTheme.TypeScale.title, weight: .bold))
          .foregroundStyle(MacTheme.ink)
        Text(file.path)
          .font(MacTheme.mono(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.secondary)
      }
      Spacer()
      Button("Done", action: { dismiss() }).buttonStyle(.paneSecondary)
    }
  }

  private var versionList: some View {
    VStack(spacing: MacTheme.Space.x6) {
      if loading {
        ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
      } else if versions.isEmpty {
        Text("No saved versions yet")
          .font(MacTheme.ui(MacTheme.TypeScale.caption)).foregroundStyle(MacTheme.secondary)
          .frame(maxWidth: .infinity, maxHeight: .infinity)
      } else {
        ScrollView {
          LazyVStack(spacing: MacTheme.Space.x5) {
            ForEach(versions, id: \.versionId) { version in
              versionRow(version)
            }
            if nextCursor != nil {
              Button("Load older versions") { Task { await loadMore() } }
                .font(MacTheme.ui(MacTheme.TypeScale.small, weight: .bold))
                .buttonStyle(.paneSecondary)
            }
          }
        }
      }
    }
    .frame(width: 260).padding(MacTheme.Space.x8).paneWell(radius: MacTheme.Radius.control)
  }

  private func versionRow(_ version: FileVersion) -> some View {
    Button {
      Task { await select(version) }
    } label: {
      VStack(alignment: .leading, spacing: MacTheme.Space.x3) {
        HStack {
          Text(version.author.name)
            .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .bold))
          Spacer()
          Text("r\(identifier: version.revision)").font(MacTheme.mono(MacTheme.TypeScale.micro))
        }
        Text(historyTime(version.createdAt))
          .font(MacTheme.ui(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.secondary)
        Text(byteLabel(version.byteCount))
          .font(MacTheme.mono(MacTheme.TypeScale.micro)).foregroundStyle(MacTheme.muted)
      }
      .foregroundStyle(MacTheme.ink).padding(MacTheme.Space.x8).frame(maxWidth: .infinity)
      .background(
        selected?.versionId == version.versionId ? MacTheme.well : MacTheme.face,
        in: RoundedRectangle(cornerRadius: 8))
    }
    .buttonStyle(.plain)
  }

  private var footer: some View {
    HStack {
      if let problem {
        Text(problem).font(MacTheme.ui(MacTheme.TypeScale.small)).foregroundStyle(
          MacTheme.danger)
      }
      Spacer()
      Button("Restore this version") { Task { await restore() } }
        .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .bold))
        .buttonStyle(.panePrimary).disabled(selected == nil)
    }
  }

  private var previewTitle: String {
    selected.map { "Revision \($0.revision) · \($0.author.name)" } ?? "Select a version"
  }

  private func loadFirstPage() async {
    loading = true
    defer { loading = false }
    do {
      let page = try await client.editorHistory(file.fileId)
      versions = page.items
      nextCursor = page.nextCursor
      if let first = versions.first { await select(first) }
    } catch { problem = error.localizedDescription }
  }

  private func loadMore() async {
    guard let cursor = nextCursor else { return }
    do {
      let page = try await client.editorHistory(file.fileId, cursor: cursor)
      versions.append(contentsOf: page.items)
      nextCursor = page.nextCursor
    } catch { problem = error.localizedDescription }
  }

  private func select(_ version: FileVersion) async {
    selected = version
    preview = nil
    do {
      preview = try await client.previewEditorFile(
        fileId: file.fileId, source: .history, versionId: version.versionId)
    } catch { problem = error.localizedDescription }
  }

  private func restore() async {
    guard let version = selected else { return }
    do {
      _ = try await client.revertEditorFile(
        file.fileId, versionId: version.versionId,
        baseRevision: client.editorFiles[file.fileId]?.file.revision ?? file.revision)
      dismiss()
    } catch { problem = error.localizedDescription }
  }
}

struct EditorConflictSheet: View {
  @Bindable var client: EngineClient
  let file: SoyehtProtocol.FileDocument
  @Environment(\.dismiss) private var dismiss
  @State private var draft: FileReadResult?
  @State private var disk: FileReadResult?
  @State private var problem: String?

  var body: some View {
    VStack(spacing: MacTheme.Space.x12) {
      conflictTitle
      HStack(spacing: MacTheme.Space.x10) {
        EditorVersionPreview(
          title: "Your draft · r\(file.revision)", viewport: draft,
          placeholder: "Loading preserved draft")
        EditorVersionPreview(
          title: "On disk · r\(file.diskRevision)", viewport: disk,
          placeholder: "Loading disk version")
      }
      conflictFooter
    }
    .padding(MacTheme.Space.x14).macSheetSurface(.editorConflict).background(MacTheme.canvas)
    .task(id: file.diskRevision) { await load() }
  }

  private var conflictTitle: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Image(systemName: "exclamationmark.triangle.fill")
        .font(.system(size: 14, weight: .semibold)).foregroundStyle(MacTheme.danger)
      VStack(alignment: .leading, spacing: MacTheme.Space.x3) {
        Text("Choose how to continue")
          .font(MacTheme.ui(MacTheme.TypeScale.title, weight: .bold))
          .foregroundStyle(MacTheme.ink)
        Text("The disk changed after your draft. Both versions are preserved.")
          .font(MacTheme.ui(MacTheme.TypeScale.caption)).foregroundStyle(MacTheme.secondary)
      }
      Spacer()
    }
  }

  private var conflictFooter: some View {
    HStack {
      if let problem {
        Text(problem).font(MacTheme.ui(MacTheme.TypeScale.small)).foregroundStyle(
          MacTheme.danger)
      }
      Spacer()
      Button("Return to editing") { dismiss() }.buttonStyle(.paneSecondary)
      Button("Keep my draft") { Task { await keepDraft() } }
        .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .bold))
        .buttonStyle(.panePrimary).disabled(draft == nil || disk == nil)
    }
  }

  private func load() async {
    do {
      draft = try await client.previewEditorFile(fileId: file.fileId, source: .draft)
      disk = try await client.previewEditorFile(fileId: file.fileId, source: .disk)
    } catch { problem = error.localizedDescription }
  }

  private func keepDraft() async {
    do {
      _ = try await client.keepEditorDraft(fileId: file.fileId, baseRevision: file.revision)
      dismiss()
    } catch { problem = error.localizedDescription }
  }
}

private struct EditorVersionPreview: View {
  let title: String
  let viewport: FileReadResult?
  let placeholder: String

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x6) {
      Text(title)
        .font(MacTheme.ui(MacTheme.TypeScale.caption, weight: .bold))
        .foregroundStyle(MacTheme.ink)
      if let viewport {
        ScrollView([.horizontal, .vertical]) {
          Text(EditorEditing.viewportText(viewport.lines))
            .font(MacTheme.mono(11.5)).foregroundStyle(MacTheme.ink)
            .textSelection(.enabled).frame(maxWidth: .infinity, alignment: .topLeading)
        }
        if viewport.toLine < viewport.file.lineCount {
          Text(
            "Preview limited to \(identifier: viewport.toLine) of \(viewport.file.lineCount) lines"
          )
          .font(MacTheme.ui(MacTheme.TypeScale.micro)).foregroundStyle(MacTheme.muted)
        }
      } else {
        Text(placeholder)
          .font(MacTheme.ui(MacTheme.TypeScale.caption)).foregroundStyle(MacTheme.secondary)
          .frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    }
    .padding(MacTheme.Space.x10).frame(maxWidth: .infinity, maxHeight: .infinity)
    .paneWell(radius: MacTheme.Radius.control)
  }
}

private func historyTime(_ value: String) -> String {
  String(value.prefix(16)).replacingOccurrences(of: "T", with: " · ")
}

private func byteLabel(_ value: String) -> String {
  guard let bytes = Int64(value) else { return "\(value) bytes" }
  return ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
}
