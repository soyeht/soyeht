import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

/// The project panel: the workspace folder as a lazy tree, like Zed's. The
/// tree model and every file operation live in `SoyehtClient` and the engine;
/// this view draws rows, edits names inline and maps keys and clicks.
struct EditorProjectPanel: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  /// The file of the active tab, revealed whenever it changes.
  let activePath: String?
  @State private var editing: EditorProjectEdit?
  @State private var draftName = ""
  @State private var confirmDelete: String?
  @FocusState private var treeFocused: Bool
  @FocusState private var nameFocused: Bool

  private var tree: EditorProjectTree { client.projectTree(workspaceId) }

  var body: some View {
    VStack(spacing: MacTheme.Space.x2) {
      header
      ScrollViewReader { proxy in
        ScrollView {
          LazyVStack(spacing: 0) {
            if case .create(let parent, let kind) = editing, parent == EditorProjectTree.root {
              nameField(depth: 0, folder: kind == .directory)
            }
            ForEach(tree.rows) { row in
              rowView(row)
              if case .create(let parent, let kind) = editing, parent == row.node.path {
                nameField(depth: row.depth + 1, folder: kind == .directory)
              }
            }
          }
        }
        .scrollIndicators(.never)
        .onChange(of: tree.selection) { _, selection in
          if let selection { proxy.scrollTo(selection) }
        }
      }
      Spacer(minLength: 0)
    }
    .padding(.vertical, MacTheme.Space.x10).padding(.horizontal, MacTheme.Space.x8)
    .frame(width: EditorProjectMetrics.width).background(MacTheme.canvas)
    .focusable()
    .focused($treeFocused)
    .focusEffectDisabled()
    .onKeyPress(phases: .down) { press in handleKey(press) }
    .task(id: "\(workspaceId):\(client.connectionRevision)") {
      await client.synchronizeProjectTree(workspaceId)
      if let activePath { await client.revealProjectEntry(workspaceId, path: activePath) }
    }
    .onChange(of: activePath) { _, path in
      guard let path else { return }
      Task { await client.revealProjectEntry(workspaceId, path: path) }
    }
    .alert(
      "Delete \(confirmDelete.map(fileName) ?? "item") permanently?",
      isPresented: Binding(get: { confirmDelete != nil }, set: { if !$0 { confirmDelete = nil } })
    ) {
      Button("Cancel", role: .cancel) { confirmDelete = nil }
      Button("Delete", role: .destructive) {
        if let path = confirmDelete { delete(path, permanent: true) }
        confirmDelete = nil
      }
    } message: {
      Text("The engine host has no trash for this item, so it cannot be put back.")
    }
  }

  private var header: some View {
    HStack(spacing: MacTheme.Space.x6) {
      Text("FILES").font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .bold))
        .tracking(EditorProjectMetrics.headerTracking).foregroundStyle(MacTheme.secondary)
      Spacer()
      headerButton("doc.badge.plus", help: "New file") { beginCreate(.file) }
      headerButton("folder.badge.plus", help: "New folder") { beginCreate(.directory) }
      headerButton(tree.showIgnored ? "eye" : "eye.slash", help: ignoredHelp) {
        Task { await client.setProjectTreeShowsIgnored(workspaceId, !tree.showIgnored) }
      }
      headerButton("scope", help: "Reveal the active file") {
        guard let activePath else { return }
        Task { await client.revealProjectEntry(workspaceId, path: activePath) }
      }
    }
    .padding(.horizontal, MacTheme.Space.x6).frame(height: EditorProjectMetrics.rowHeight)
  }

  private var ignoredHelp: String {
    tree.showIgnored ? "Hide files ignored by Git" : "Show files ignored by Git"
  }

  private func headerButton(_ symbol: String, help: String, action: @escaping () -> Void)
    -> some View
  {
    Button(action: action) {
      Image(systemName: symbol)
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .medium))
    }
    .buttonStyle(.plain).foregroundStyle(MacTheme.muted).help(help)
    .disabled(!client.connected)
  }

  @ViewBuilder private func rowView(_ row: EditorProjectTree.Row) -> some View {
    if case .rename(let path) = editing, path == row.node.path {
      nameField(depth: row.depth, folder: row.node.isDirectory)
    } else {
      EditorProjectRow(
        row: row, selected: tree.selection == row.node.path,
        active: activePath == row.node.path
      )
      .id(row.node.path)
      .onTapGesture(count: 2) { open(row.node, preview: false) }
      .onTapGesture { click(row.node) }
      .overlay(EditorMiddleClickArea { open(row.node, preview: false) })
      .contextMenu { menu(for: row.node) }
    }
  }

  @ViewBuilder private func menu(for node: EditorProjectTree.Node) -> some View {
    if !node.isDirectory {
      Button("Open") { open(node, preview: false) }
      Divider()
    }
    Button("New File") {
      select(node.path)
      beginCreate(.file)
    }
    Button("New Folder") {
      select(node.path)
      beginCreate(.directory)
    }
    Divider()
    Button("Rename") { beginRename(node.path) }
    Button("Move to Trash") { delete(node.path, permanent: false) }
    Divider()
    Button("Copy Relative Path") {
      NSPasteboard.general.clearContents()
      NSPasteboard.general.setString(node.path, forType: .string)
    }
  }

  private func nameField(depth: Int, folder: Bool) -> some View {
    HStack(spacing: MacTheme.Space.x6) {
      Image(systemName: folder ? "folder" : "doc.text")
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .medium))
        .foregroundStyle(MacTheme.secondary)
      TextField("Name", text: $draftName)
        .textFieldStyle(.plain)
        .font(MacTheme.mono(EditorProjectMetrics.nameFont))
        .foregroundStyle(MacTheme.ink)
        .focused($nameFocused)
        .onSubmit(commitName)
        .onExitCommand { editing = nil }
    }
    .padding(.leading, CGFloat(depth) * EditorProjectMetrics.indent)
    .padding(.horizontal, MacTheme.Space.x8).frame(height: EditorProjectMetrics.rowHeight)
    .background(MacTheme.well, in: RoundedRectangle(cornerRadius: MacTheme.Radius.compact))
    .onAppear { nameFocused = true }
  }

  private func click(_ node: EditorProjectTree.Node) {
    select(node.path)
    treeFocused = true
    if node.isDirectory {
      Task { await client.updateProjectTree(workspaceId) { _ = $0.toggle(node.path) } }
    } else {
      open(node, preview: true)
    }
  }

  private func select(_ path: String) {
    Task { await client.updateProjectTree(workspaceId) { $0.selection = path } }
  }

  /// A single click previews, a double click or Return keeps the tab, as in Zed.
  private func open(_ node: EditorProjectTree.Node, preview: Bool) {
    guard !node.isDirectory else { return }
    Task { @MainActor in
      do {
        try await client.openEditorFile(
          workspaceId: workspaceId, path: node.path, preview: preview)
      } catch { client.problem = error.localizedDescription }
    }
  }

  private func beginCreate(_ kind: ProjectEntryKind) {
    let parent = tree.creationDirectory
    draftName = ""
    editing = .create(parent: parent, kind: kind)
    if parent != EditorProjectTree.root {
      Task { await client.updateProjectTree(workspaceId) { $0.expand(parent) } }
    }
  }

  private func beginRename(_ path: String) {
    draftName = fileName(path)
    editing = .rename(path: path)
  }

  private func commitName() {
    let name = draftName.trimmingCharacters(in: .whitespacesAndNewlines)
    guard let edit = editing, !name.isEmpty else {
      editing = nil
      return
    }
    editing = nil
    treeFocused = true
    Task { @MainActor in
      do {
        switch edit {
        case .create(let parent, let kind):
          let entry = try await client.createProjectEntry(
            workspaceId: workspaceId, path: EditorProjectTree.join(parent, name), kind: kind)
          if kind == .file {
            try await client.openEditorFile(workspaceId: workspaceId, path: entry.path)
          }
        case .rename(let path):
          let target = EditorProjectTree.join(EditorProjectTree.parent(of: path), name)
          guard target != path else { return }
          try await client.renameProjectEntry(workspaceId: workspaceId, from: path, to: target)
        }
      } catch { client.problem = error.localizedDescription }
    }
  }

  /// Moves to the trash without asking, since the item can be put back; asks
  /// only when the engine host has no trash.
  private func delete(_ path: String, permanent: Bool) {
    Task { @MainActor in
      do {
        let outcome = try await client.deleteProjectEntry(
          workspaceId: workspaceId, path: path, permanent: permanent)
        if outcome == .needs_confirmation { confirmDelete = path }
      } catch { client.problem = error.localizedDescription }
    }
  }

  private func handleKey(_ press: KeyPress) -> KeyPress.Result {
    guard editing == nil else { return .ignored }
    let key: EditorProjectTree.Key?
    switch press.key {
    case .upArrow: key = .up
    case .downArrow: key = .down
    case .leftArrow: key = .left
    case .rightArrow: key = .right
    case .home: key = .first
    case .end: key = .last
    default: key = nil
    }
    if let key {
      Task { await client.updateProjectTree(workspaceId) { $0.handle(key) } }
      return .handled
    }
    guard let path = tree.selection, let node = tree.node(at: path) else { return .ignored }
    switch press.key {
    case .return:
      if node.isDirectory {
        Task { await client.updateProjectTree(workspaceId) { _ = $0.toggle(path) } }
      } else {
        open(node, preview: false)
      }
    case .space: open(node, preview: true)
    case .delete where press.modifiers.contains(.command): delete(path, permanent: false)
    default:
      // F2 renames, as in Zed.
      guard press.characters == "\u{F705}" else { return .ignored }
      beginRename(path)
    }
    return .handled
  }

  private func fileName(_ path: String) -> String {
    String(path.split(separator: "/").last ?? Substring(path))
  }
}

enum EditorProjectEdit: Equatable {
  case create(parent: String, kind: ProjectEntryKind)
  case rename(path: String)
}

enum EditorProjectMetrics {
  static let width = CGFloat(PaneLayoutContentMetrics.editorProjectPanelWidth)
  static let rowHeight: CGFloat = 22
  static let indent: CGFloat = 12
  static let nameFont: CGFloat = 11.5
  static let headerTracking: CGFloat = 0.8
}

private struct EditorProjectRow: View {
  let row: EditorProjectTree.Row
  let selected: Bool
  let active: Bool

  var body: some View {
    HStack(spacing: MacTheme.Space.x4) {
      Image(systemName: row.expanded ? "chevron.down" : "chevron.right")
        .font(MacTheme.chrome(MacTheme.TypeScale.micro, weight: .semibold))
        .foregroundStyle(MacTheme.muted)
        .opacity(row.node.isDirectory ? 1 : 0)
        .frame(width: EditorProjectMetrics.indent)
      Image(systemName: row.node.isDirectory ? "folder" : "doc.text")
        .font(MacTheme.chrome(MacTheme.TypeScale.small, weight: .medium))
        .foregroundStyle(active ? MacTheme.accent : MacTheme.secondary)
      Text(row.node.name)
        .font(MacTheme.mono(EditorProjectMetrics.nameFont, weight: active ? .bold : .regular))
        .foregroundStyle(textColor)
        .lineLimit(1).truncationMode(.middle)
      Spacer(minLength: 0)
    }
    .padding(.leading, CGFloat(row.depth) * EditorProjectMetrics.indent)
    .padding(.horizontal, MacTheme.Space.x6).frame(height: EditorProjectMetrics.rowHeight)
    .background(
      selected ? MacTheme.well : Color.clear,
      in: RoundedRectangle(cornerRadius: MacTheme.Radius.compact)
    )
    .contentShape(Rectangle())
    .help(row.node.ignored ? "\(row.node.path) · ignored by Git" : row.node.path)
    .accessibilityElement(children: .combine)
    .accessibilityAddTraits(selected ? .isSelected : [])
  }

  private var textColor: Color {
    if active { return MacTheme.accent }
    return row.node.ignored ? MacTheme.muted : MacTheme.ink
  }
}

/// The editor surface before any file is open: the project panel to browse,
/// beside the system open panel for files outside it.
struct EditorEmptySurface: View {
  @Bindable var client: EngineClient
  let workspaceId: String
  let openFile: () -> Void

  var body: some View {
    HStack(spacing: 0) {
      EditorProjectPanel(client: client, workspaceId: workspaceId, activePath: nil)
      VStack(spacing: MacTheme.Space.x18) {
        Image(systemName: "doc.text")
          .font(MacTheme.chrome(MacTheme.TypeScale.title, weight: .light))
          .foregroundStyle(MacTheme.secondary)
        Text("Open a file to begin")
          .font(MacTheme.chrome(MacTheme.TypeScale.heading, weight: .bold))
        Text("Pick one in Files, or press ⌘P to find it by name.")
          .font(MacTheme.chrome(MacTheme.TypeScale.caption))
          .foregroundStyle(MacTheme.secondary)
        Button("Open file", action: openFile)
          .buttonStyle(NeoPillButtonStyle(accent: true))
          .disabled(!client.connected)
      }
      .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
    .background(MacTheme.face)
    .clipShape(RoundedRectangle(cornerRadius: MacTheme.Radius.control))
  }
}
