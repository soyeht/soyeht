import Foundation
import SoyehtClient
import SoyehtProtocol
import SwiftUI

struct WritingSectionView: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  let section: WritingSection
  let onOpen: (String) -> Void
  @State private var entries: [WritingSectionEntry] = []
  @State private var nextCursor: String?
  @State private var query = ""
  @State private var showingCreate = false
  @State private var loading = false

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x14) {
      header
      controls
      content
    }
    .padding(MacTheme.Space.x18)
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    .background(MacTheme.canvas)
    .task(id: refreshKey) { await reload() }
    .sheet(isPresented: $showingCreate) {
      WritingDocumentCreateSheet(client: client, workspaceId: workspaceId, section: section) {
        showingCreate = false
        onOpen($0)
      }
    }
  }

  private var header: some View {
    HStack {
      VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
        Text(section.title)
          .font(MacTheme.chrome(MacTheme.TypeScale.title, weight: .bold))
        Text(section.subtitle)
          .font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary)
      }
      Spacer()
      Button {
        showingCreate = true
      } label: {
        Label("New document", systemImage: "plus")
          .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
          .padding(.horizontal, MacTheme.Space.x12)
          .frame(height: 34)
          .neoRaised(radius: MacTheme.Radius.pill, elevation: .pill)
      }
      .accessibilityIdentifier("writing-new-document")
    }
  }

  private var controls: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Image(systemName: "magnifyingglass")
        .font(.system(size: 13, weight: .semibold))
        .foregroundStyle(MacTheme.secondary)
      TextField("Search this section", text: $query)
        .textFieldStyle(.plain)
        .font(MacTheme.chrome(MacTheme.TypeScale.control))
    }
    .padding(.horizontal, MacTheme.Space.x12)
    .frame(height: 36)
    .neoWell(radius: MacTheme.Radius.control)
  }

  @ViewBuilder private var content: some View {
    if loading && entries.isEmpty {
      ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
    } else if filteredEntries.isEmpty {
      emptyState
    } else {
      ScrollView {
        LazyVStack(alignment: .leading, spacing: MacTheme.Space.x8) {
          ForEach(filteredEntries, id: \.path) { entry in
            WritingSectionRow(entry: entry) { open(entry) }
          }
          if nextCursor != nil {
            Button("Load more") { Task { await loadMore() } }
              .font(MacTheme.chrome(MacTheme.TypeScale.control, weight: .bold))
              .frame(maxWidth: .infinity)
              .padding(.top, MacTheme.Space.x8)
          }
        }
        .padding(MacTheme.Space.x4)
      }
    }
  }

  private var emptyState: some View {
    VStack(spacing: MacTheme.Space.x12) {
      Image(systemName: section.icon)
        .font(.system(size: 38, weight: .light))
        .foregroundStyle(MacTheme.secondary)
      Text(query.isEmpty ? "No documents yet" : "No matching documents")
        .font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
      if query.isEmpty {
        Text("Create one here and it stays in the \(section.folderLabel) folder.")
          .font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary)
      }
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
  }

  private var filteredEntries: [WritingSectionEntry] {
    let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !needle.isEmpty else { return entries }
    return entries.filter {
      $0.title.localizedCaseInsensitiveContains(needle)
        || $0.path.localizedCaseInsensitiveContains(needle)
    }
  }

  private var refreshKey: String { "\(workspaceId):\(section.rawValue)" }

  private func reload() async {
    loading = true
    defer { loading = false }
    do {
      let result = try await client.listWritingSection(
        workspaceId: workspaceId, section: section)
      entries = result.entries.filter { $0.section == section && $0.kind == section.writingKind }
      nextCursor = result.nextCursor
    } catch { client.problem = error.localizedDescription }
  }

  private func loadMore() async {
    guard !loading, let cursor = nextCursor else { return }
    loading = true
    defer { loading = false }
    do {
      let result = try await client.listWritingSection(
        workspaceId: workspaceId, section: section, cursor: cursor)
      let accepted = result.entries.filter {
        $0.section == section && $0.kind == section.writingKind
      }
      entries.append(
        contentsOf: accepted.filter { candidate in
          !entries.contains(where: { $0.path == candidate.path })
        })
      nextCursor = result.nextCursor
    } catch { client.problem = error.localizedDescription }
  }

  private func open(_ entry: WritingSectionEntry) {
    Task { @MainActor in
      do {
        let result = try await client.openWritingDocument(
          workspaceId: workspaceId, path: entry.path, kind: section.writingKind)
        onOpen(result.pane.paneId)
      } catch { client.problem = error.localizedDescription }
    }
  }
}

private struct WritingSectionRow: View {
  let entry: WritingSectionEntry
  let action: () -> Void

  var body: some View {
    Button(action: action) {
      HStack(spacing: MacTheme.Space.x12) {
        Image(systemName: entry.section.icon)
          .font(.system(size: 17, weight: .medium))
          .foregroundStyle(MacTheme.accent)
          .frame(width: 34, height: 34)
          .neoWell(radius: MacTheme.Radius.compact)
        VStack(alignment: .leading, spacing: MacTheme.Space.x4) {
          Text(entry.title)
            .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
            .lineLimit(1)
          Text(entry.path)
            .font(MacTheme.mono(MacTheme.TypeScale.small))
            .foregroundStyle(MacTheme.secondary)
            .lineLimit(1)
        }
        Spacer()
        Text(byteCountLabel)
          .font(MacTheme.chrome(MacTheme.TypeScale.small))
          .foregroundStyle(MacTheme.muted)
        Image(systemName: "chevron.right")
          .font(.system(size: 11, weight: .bold))
          .foregroundStyle(MacTheme.secondary)
      }
      .padding(MacTheme.Space.x12)
      .neoRaised(radius: MacTheme.Radius.control, elevation: .taskCard)
    }
    .accessibilityIdentifier("writing-document-\(entry.path)")
  }

  private var byteCountLabel: String {
    guard let bytes = Int64(entry.byteCount), bytes > 0 else { return "0 kB" }
    return bytes.formatted(.byteCount(style: .file))
  }
}

private struct WritingDocumentCreateSheet: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  let section: WritingSection
  let onCreated: (String) -> Void
  @Environment(\.dismiss) private var dismiss
  @State private var title = ""
  @State private var folder = ""
  @State private var creating = false

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x16) {
      Text("New \(section.singularTitle)")
        .font(MacTheme.chrome(MacTheme.TypeScale.title, weight: .bold))
      TextField("Title", text: $title)
        .textFieldStyle(.plain).font(MacTheme.chrome(MacTheme.TypeScale.control))
        .padding(MacTheme.Space.x10).neoWell(radius: MacTheme.Radius.control)
      TextField("Subfolder (optional)", text: $folder)
        .textFieldStyle(.plain).font(MacTheme.mono(MacTheme.TypeScale.control))
        .padding(MacTheme.Space.x10).neoWell(radius: MacTheme.Radius.control)
      Text("The document is created inside the \(section.folderLabel) folder.")
        .font(MacTheme.chrome(MacTheme.TypeScale.caption)).foregroundStyle(MacTheme.secondary)
      HStack {
        Button("Cancel") { dismiss() }
        Spacer()
        Button("Create") { create() }
          .disabled(!valid || creating)
          .buttonStyle(.borderedProminent)
      }
    }
    .padding(MacTheme.Space.x18)
    .macSheetSurface(.writingCreate)
    .background(MacTheme.canvas)
  }

  private var valid: Bool { !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

  private func create() {
    creating = true
    Task { @MainActor in
      do {
        let result = try await client.createWritingDocument(
          workspaceId: workspaceId, section: section,
          title: title.trimmingCharacters(in: .whitespacesAndNewlines),
          relativeFolder: folder.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty)
        onCreated(result.pane.paneId)
      } catch {
        creating = false
        client.problem = error.localizedDescription
      }
    }
  }
}

extension WritingSection {
  fileprivate var title: String {
    switch self {
    case .texts: "Texts"
    case .presentations: "Presentations"
    case .screenplays: "Screenplays"
    }
  }
  fileprivate var singularTitle: String {
    switch self {
    case .texts: "text"
    case .presentations: "presentation"
    case .screenplays: "screenplay"
    }
  }
  fileprivate var subtitle: String {
    switch self {
    case .texts: "Essays, notes, and long-form Markdown"
    case .presentations: "Slide documents with speaker notes"
    case .screenplays: "Fountain scripts and scene plans"
    }
  }
  fileprivate var icon: String {
    switch self {
    case .texts: "text.page"
    case .presentations: "rectangle.on.rectangle"
    case .screenplays: "movieclapper"
    }
  }
  fileprivate var folderLabel: String {
    switch self {
    case .texts: "texts"
    case .presentations: "presentations"
    case .screenplays: "screenplays"
    }
  }
}

extension String {
  fileprivate var nilIfEmpty: String? { isEmpty ? nil : self }
}
