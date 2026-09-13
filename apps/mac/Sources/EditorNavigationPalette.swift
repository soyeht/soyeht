import AppKit
import SoyehtClient
import SoyehtProtocol
import SwiftUI

enum EditorPaletteMode: String, Sendable {
  case commands
  case files
  case everywhere
  case project
  case line

  var title: String {
    switch self {
    case .commands: "Command Palette"
    case .files: "Open File"
    case .everywhere: "Search Everywhere"
    case .project: "Search Project"
    case .line: "Go to Line"
    }
  }

  var placeholder: String {
    switch self {
    case .commands: "Type a command"
    case .files: "Type a file name"
    case .everywhere: "Search files, code, commands, and settings"
    case .project: "Search project contents"
    case .line: "Line number"
    }
  }
}

struct EditorNavigationPalette: View {
  @Bindable var client: EngineClient
  let workspaceId: String?
  @Binding var mode: EditorPaletteMode?
  @State private var query = ""
  @State private var replacement = ""
  @State private var matches: [FileSearchMatch] = []
  @State private var symbols: [FileSymbol] = []
  @State private var status: String?
  @FocusState private var queryFocused: Bool

  var body: some View {
    VStack(alignment: .leading, spacing: MacTheme.Space.x10) {
      header
      TextField(activeMode.placeholder, text: $query)
        .textFieldStyle(.plain)
        .font(MacTheme.mono(MacTheme.TypeScale.control))
        .padding(.horizontal, MacTheme.Space.x12).frame(height: 36)
        .neoWell(radius: MacTheme.Radius.control)
        .focused($queryFocused)
        .onSubmit { submitPrimary() }
      if activeMode == .project { replacementField }
      resultList
      if let status {
        Text(status).font(MacTheme.chrome(MacTheme.TypeScale.small))
          .foregroundStyle(MacTheme.secondary)
      }
    }
    .padding(MacTheme.Space.x14).frame(width: 620).frame(maxHeight: 520)
    .neoRaised(radius: MacTheme.Radius.card, elevation: .panel)
    .task(id: searchKey) { await updateResults() }
    .onAppear { queryFocused = true }
    .onExitCommand { mode = nil }
    .accessibilityIdentifier("editor-navigation-palette")
  }

  private var header: some View {
    HStack(spacing: MacTheme.Space.x8) {
      Image(systemName: activeMode == .commands ? "command" : "magnifyingglass")
        .font(.system(size: 13, weight: .semibold)).foregroundStyle(MacTheme.accent)
      Text(activeMode.title)
        .font(MacTheme.chrome(MacTheme.TypeScale.subheading, weight: .bold))
      Spacer()
      Text("esc").font(MacTheme.mono(MacTheme.TypeScale.micro, weight: .bold))
        .foregroundStyle(MacTheme.secondary)
    }
  }

  private var replacementField: some View {
    HStack(spacing: MacTheme.Space.x8) {
      TextField("Replace with", text: $replacement)
        .textFieldStyle(.plain).font(MacTheme.mono(MacTheme.TypeScale.control))
        .padding(.horizontal, MacTheme.Space.x12).frame(height: 34)
        .neoWell(radius: MacTheme.Radius.control)
      Button("Replace all") { replaceAll() }
        .buttonStyle(NeoPillButtonStyle(accent: true)).disabled(query.isEmpty)
    }
  }

  @ViewBuilder private var resultList: some View {
    ScrollView {
      LazyVStack(spacing: MacTheme.Space.x4) {
        if activeMode == .commands || activeMode == .everywhere {
          ForEach(filteredCommands) { command in
            commandRow(command)
          }
        }
        ForEach(Array(symbols.enumerated()), id: \.offset) { _, symbol in
          symbolRow(symbol)
        }
        ForEach(matches, id: \.paletteId) { match in
          matchRow(match)
        }
        if filteredCommands.isEmpty && symbols.isEmpty && matches.isEmpty && !query.isEmpty {
          Text("No matches")
            .font(MacTheme.chrome(MacTheme.TypeScale.caption))
            .foregroundStyle(MacTheme.secondary).padding(.vertical, MacTheme.Space.x18)
        }
      }
    }
    .frame(minHeight: 80, maxHeight: 350)
  }

  private func commandRow(_ command: EditorCommand) -> some View {
    Button {
      run(command)
    } label: {
      HStack(spacing: MacTheme.Space.x10) {
        Image(systemName: command.icon).font(.system(size: 13, weight: .medium))
          .frame(width: 20).foregroundStyle(MacTheme.accent)
        VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
          Text(command.title).font(MacTheme.chrome(MacTheme.TypeScale.caption, weight: .bold))
          Text(command.hint).font(MacTheme.chrome(MacTheme.TypeScale.small))
            .foregroundStyle(MacTheme.secondary)
        }
        Spacer()
        Text(command.shortcut).font(MacTheme.mono(MacTheme.TypeScale.small))
          .foregroundStyle(MacTheme.muted)
      }
      .padding(.horizontal, MacTheme.Space.x10).frame(height: 46)
      .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    }
  }

  private func matchRow(_ match: FileSearchMatch) -> some View {
    Button {
      open(match)
    } label: {
      HStack(spacing: MacTheme.Space.x10) {
        Image(systemName: "doc.text").font(.system(size: 13, weight: .medium))
          .frame(width: 20).foregroundStyle(MacTheme.secondary)
        VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
          Text(match.path).font(MacTheme.mono(MacTheme.TypeScale.caption, weight: .semibold))
            .lineLimit(1)
          if !match.preview.isEmpty {
            Text(match.preview).font(MacTheme.mono(MacTheme.TypeScale.small))
              .foregroundStyle(MacTheme.secondary).lineLimit(1)
          }
        }
        Spacer()
        Text("\(UInt64(match.range.start.line) + 1):\(UInt64(match.range.start.column) + 1)")
          .font(MacTheme.mono(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.muted)
      }
      .padding(.horizontal, MacTheme.Space.x10).frame(height: 46)
      .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    }
  }

  private func symbolRow(_ symbol: FileSymbol) -> some View {
    Button {
      open(symbol)
    } label: {
      HStack(spacing: MacTheme.Space.x10) {
        Image(systemName: symbolIcon(symbol.kind)).font(.system(size: 13, weight: .medium))
          .frame(width: 20).foregroundStyle(MacTheme.accent)
        VStack(alignment: .leading, spacing: MacTheme.Space.x2) {
          Text(symbol.name).font(MacTheme.mono(MacTheme.TypeScale.caption, weight: .semibold))
          Text("Symbol · \(symbol.kind.rawValue)")
            .font(MacTheme.chrome(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.secondary)
        }
        Spacer()
        Text("\(UInt64(symbol.selectionRange.start.line) + 1)")
          .font(MacTheme.mono(MacTheme.TypeScale.small)).foregroundStyle(MacTheme.muted)
      }
      .padding(.horizontal, MacTheme.Space.x10).frame(height: 46)
      .background(MacTheme.face, in: RoundedRectangle(cornerRadius: MacTheme.Radius.control))
    }
  }

  private var activeMode: EditorPaletteMode { mode ?? .commands }
  private var searchKey: String { "\(activeMode.rawValue):\(query)" }

  private var filteredCommands: [EditorCommand] {
    guard activeMode == .commands || activeMode == .everywhere else { return [] }
    guard !query.isEmpty else { return EditorCommand.allCases }
    return EditorCommand.allCases.filter { fuzzyContains($0.title + " " + $0.hint, query) }
  }

  private func updateResults() async {
    matches = []
    symbols = []
    status = nil
    guard let workspaceId, !query.isEmpty else { return }
    do {
      try await Task.sleep(for: .milliseconds(120))
      guard !Task.isCancelled else { return }
      switch activeMode {
      case .files:
        matches = try await search(workspaceId, scope: .path)
      case .project:
        matches = try await search(workspaceId, scope: .content)
      case .everywhere:
        let pathMatches = try await search(workspaceId, scope: .path)
        let contentMatches = try await search(workspaceId, scope: .content)
        matches = Array((pathMatches + contentMatches).prefix(ProtocolLimits.editorSearchResults))
        symbols = await matchingSymbols()
      case .commands, .line: return
      }
    } catch is CancellationError {
      return
    } catch {
      status = error.localizedDescription
    }
  }

  private func matchingSymbols() async -> [FileSymbol] {
    guard let file = client.selectedEditorFile?.file,
      let result = try? await client.editorSymbols(fileId: file.fileId, revision: file.revision)
    else { return [] }
    return result.items.filter { fuzzyContains($0.name, query) }
  }

  private func search(_ workspaceId: String, scope: FileSearchScope) async throws
    -> [FileSearchMatch]
  {
    try await client.searchEditorFiles(
      workspaceId: workspaceId, scope: scope, query: query
    ).items
  }

  private func submitPrimary() {
    if let command = filteredCommands.first, symbols.isEmpty, matches.isEmpty {
      run(command)
    } else if let symbol = symbols.first {
      open(symbol)
    } else if let match = matches.first {
      open(match)
    } else if activeMode == .line {
      goToLine()
    }
  }

  private func run(_ command: EditorCommand) {
    switch command {
    case .openFile: reset(to: .files)
    case .searchProject: reset(to: .project)
    case .searchEverywhere: reset(to: .everywhere)
    case .goToLine: reset(to: .line)
    case .formatDocument: formatDocument()
    case .settings:
      mode = nil
      NotificationCenter.default.post(name: .showSoyehtPreferences, object: nil)
    case .toggleAppBar:
      mode = nil
      NotificationCenter.default.post(name: .toggleSoyehtAppBar, object: nil)
    }
  }

  private func reset(to nextMode: EditorPaletteMode) {
    query = ""
    replacement = ""
    matches = []
    symbols = []
    status = nil
    mode = nextMode
    queryFocused = true
  }

  private func open(_ match: FileSearchMatch) {
    Task { @MainActor in
      do {
        _ = try await client.openEditorFile(
          workspaceId: match.workspaceId, path: match.path, line: match.range.start.line,
          column: match.range.start.column)
        mode = nil
      } catch { status = error.localizedDescription }
    }
  }

  private func open(_ symbol: FileSymbol) {
    guard let file = client.selectedEditorFile?.file else { return }
    Task { @MainActor in
      do {
        _ = try await client.readEditorFile(
          fileId: file.fileId, fromLine: symbol.selectionRange.start.line, lineCount: 200)
        mode = nil
      } catch { status = error.localizedDescription }
    }
  }

  private func symbolIcon(_ kind: FileSymbolKind) -> String {
    switch kind {
    case .function, .method: "function"
    case .type, .module: "shippingbox"
    case .property, .variable: "textformat"
    }
  }

  private func goToLine() {
    guard let line = UInt32(query), line > 0, let file = client.selectedEditorFile else {
      status = "Enter a line number in an open editor file"
      return
    }
    Task { @MainActor in
      do {
        _ = try await client.readEditorFile(
          fileId: file.file.fileId, fromLine: line - 1, lineCount: 200)
        mode = nil
      } catch { status = error.localizedDescription }
    }
  }

  private func replaceAll() {
    guard let workspaceId else { return }
    Task { @MainActor in
      do {
        let result = try await client.searchEditorFiles(
          workspaceId: workspaceId, query: query, replacement: replacement)
        status = "Replaced \(result.replaced) matches"
        matches = result.items
      } catch { status = error.localizedDescription }
    }
  }

  private func formatDocument() {
    guard let file = client.selectedEditorFile?.file else {
      status = "Open an editor file before formatting"
      return
    }
    Task { @MainActor in
      do {
        _ = try await client.formatEditorFile(fileId: file.fileId, baseRevision: file.revision)
        mode = nil
      } catch { status = error.localizedDescription }
    }
  }
}

private enum EditorCommand: String, CaseIterable, Identifiable {
  case openFile
  case searchProject
  case searchEverywhere
  case goToLine
  case formatDocument
  case settings
  case toggleAppBar

  var id: String { rawValue }
  var title: String {
    switch self {
    case .openFile: "Open File"
    case .searchProject: "Search Project"
    case .searchEverywhere: "Search Everywhere"
    case .goToLine: "Go to Line"
    case .formatDocument: "Format Document"
    case .settings: "Open Settings"
    case .toggleAppBar: "Toggle App Bar"
    }
  }
  var hint: String {
    switch self {
    case .openFile: "Find a workspace file by name"
    case .searchProject: "Find or replace text across the workspace"
    case .searchEverywhere: "Find files, code, commands, and settings"
    case .goToLine: "Move the visible viewport to a line"
    case .formatDocument: "Format the open file with its language server"
    case .settings: "Open app preferences"
    case .toggleAppBar: "Show or hide workspace navigation"
    }
  }
  var icon: String {
    switch self {
    case .openFile: "doc.text.magnifyingglass"
    case .searchProject, .searchEverywhere: "magnifyingglass"
    case .goToLine: "number"
    case .formatDocument: "text.alignleft"
    case .settings: "gearshape"
    case .toggleAppBar: "sidebar.left"
    }
  }
  var shortcut: String {
    switch self {
    case .openFile: "⌘P"
    case .searchProject: "⌘⇧F"
    case .searchEverywhere: "⇧⇧"
    case .goToLine: "⌃G"
    case .formatDocument: "⌥⇧F"
    case .settings: "⌘,"
    case .toggleAppBar: "⌘⇧B"
    }
  }
}

private func fuzzyContains(_ candidate: String, _ query: String) -> Bool {
  let haystack = candidate.lowercased()
  let needle = query.lowercased()
  var cursor = haystack.startIndex
  for character in needle {
    guard let match = haystack[cursor...].firstIndex(of: character) else { return false }
    cursor = haystack.index(after: match)
  }
  return true
}

extension FileSearchMatch {
  fileprivate var paletteId: String {
    "\(path):\(range.start.line):\(range.start.column)"
  }
}

struct EditorDoubleShiftMonitor: NSViewRepresentable {
  let action: @MainActor () -> Void

  func makeCoordinator() -> Coordinator { Coordinator(action: action) }

  func makeNSView(context: Context) -> NSView {
    context.coordinator.start()
    return NSView(frame: .zero)
  }

  func updateNSView(_ nsView: NSView, context: Context) {}

  static func dismantleNSView(_ nsView: NSView, coordinator: Coordinator) {
    coordinator.stop()
  }

  @MainActor final class Coordinator {
    private let action: @MainActor () -> Void
    private var monitor: Any?
    private var detector = DoubleShiftDetector()

    init(action: @escaping @MainActor () -> Void) { self.action = action }

    func start() {
      guard monitor == nil else { return }
      monitor = NSEvent.addLocalMonitorForEvents(matching: .flagsChanged) { [weak self] event in
        self?.receive(event)
        return event
      }
    }

    func stop() {
      if let monitor { NSEvent.removeMonitor(monitor) }
      monitor = nil
    }

    private func receive(_ event: NSEvent) {
      guard event.keyCode == 56 || event.keyCode == 60 else { return }
      let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
      guard flags == .shift else { return }
      if detector.registerShiftDown(at: event.timestamp) { action() }
    }
  }
}
