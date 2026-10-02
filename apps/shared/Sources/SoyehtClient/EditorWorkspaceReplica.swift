import Foundation
import SoyehtProtocol

/// Editor state that belongs to a workspace rather than to one file: the
/// engine's tab order, this client's tab activation history and the project
/// panel tree. It lives on the client so every editor pane of a workspace, on
/// the Mac or the iPad, shows the same tabs and the same expanded folders.
public struct EditorWorkspaceReplica: Sendable {
  public internal(set) var tabs: [String: [EditorTab]] = [:]
  public internal(set) var history: [String: [String]] = [:]
  public internal(set) var trees: [String: EditorProjectTree] = [:]
  var watched: [String: [String]] = [:]
  /// The connection each workspace's tabs and watches were last loaded on;
  /// watches belong to a connection, so a new one starts them over.
  var tabsConnection: [String: UInt64] = [:]
  var treeConnection: [String: UInt64] = [:]

  public init() {}
}

extension EngineClient {
  /// Tabs of a workspace in engine order. An editor pane the engine has not
  /// listed yet (its event is still in flight) follows at the end.
  public func editorTabs(in workspaceId: String) -> [EditorTab] {
    let live = panes.filter { $0.workspaceId == workspaceId && $0.mode == .editor }
    let liveIds = Set(live.map(\.paneId))
    var tabs = editorWorkspace.tabs[workspaceId, default: []].filter {
      liveIds.contains($0.paneId)
    }
    let listed = Set(tabs.map(\.paneId))
    for pane in live where !listed.contains(pane.paneId) {
      guard let file = editorFiles.values.first(where: { $0.file.paneId == pane.paneId })?.file
      else { continue }
      tabs.append(
        EditorTab(paneId: pane.paneId, fileId: file.fileId, path: file.path, preview: false))
    }
    return tabs
  }

  /// The editor pane a workspace shows in its tabbed editor surface.
  public func activeEditorPaneId(in workspaceId: String) -> String? {
    EditorTabPolicy.active(
      selected: selectedPaneId, tabs: editorTabs(in: workspaceId).map(\.paneId),
      history: editorWorkspace.history[workspaceId] ?? [])
  }

  /// The panes a grid shows for a destination. Every grid shows the editor
  /// panes as one tabbed area: only the active tab's pane is laid out.
  public func paneGridPanes(_ workspacePanes: [Pane], mode: PaneMode?) -> [Pane] {
    layoutPanes(workspacePanes).filter { mode == nil || $0.mode == mode }
  }

  public func activateEditorTab(_ paneId: String) {
    if let workspaceId = panes.first(where: { $0.paneId == paneId })?.workspaceId {
      editorWorkspace.history[workspaceId] = EditorTabPolicy.recorded(
        paneId, in: editorWorkspace.history[workspaceId] ?? [])
    }
    select(paneId)
  }

  public func cycleEditorTab(in workspaceId: String, forward: Bool) {
    let tabs = editorTabs(in: workspaceId).map(\.paneId)
    guard
      let next = EditorTabPolicy.cycled(
        from: activeEditorPaneId(in: workspaceId), tabs: tabs, forward: forward)
    else { return }
    activateEditorTab(next)
  }

  /// Loads the engine's tab order once per connection.
  public func loadEditorTabsIfNeeded(workspaceId: String) async {
    guard connected, editorWorkspace.tabsConnection[workspaceId] != connectionRevision else {
      return
    }
    editorWorkspace.tabsConnection[workspaceId] = connectionRevision
    do { try await loadEditorTabs(workspaceId: workspaceId) } catch {
      editorWorkspace.tabsConnection[workspaceId] = nil
    }
  }

  public func loadEditorTabs(workspaceId: String) async throws {
    let result = try await request(
      "editor.tabs.list", .encoded(EditorTabsListParams(workspaceId: workspaceId))
    ).decoded(EditorTabsListResult.self)
    editorWorkspace.tabs[result.workspaceId] = result.tabs
  }

  /// Closes a tab. Queued keystrokes reach the engine first, and the engine
  /// saves the draft before the pane closes, so closing never drops an edit.
  public func closeEditorTab(_ paneId: String) async throws {
    guard let pane = panes.first(where: { $0.paneId == paneId }) else { return }
    if let fileId = editorFiles.values.first(where: { $0.file.paneId == paneId })?.file.fileId {
      await settleEditorWrites(fileId)
    }
    let workspaceId = pane.workspaceId
    if activeEditorPaneId(in: workspaceId) == paneId {
      let history = editorWorkspace.history[workspaceId] ?? []
      if let next = EditorTabPolicy.successor(
        closing: paneId, tabs: editorTabs(in: workspaceId).map(\.paneId), history: history)
      {
        activateEditorTab(next)
      }
    }
    editorWorkspace.history[workspaceId]?.removeAll { $0 == paneId }
    _ = try await mutation(
      "pane.close", PaneCloseParams(paneId: paneId, idempotencyKey: UUID().uuidString))
  }

  public func moveEditorTab(workspaceId: String, paneId: String, to index: Int) async throws {
    let current = editorTabs(in: workspaceId)
    let order = EditorTabPolicy.moved(current.map(\.paneId), paneId: paneId, to: index)
    editorWorkspace.tabs[workspaceId] = order.compactMap { id in
      current.first { $0.paneId == id }
    }
    let result = try await mutation(
      "editor.tabs.move",
      EditorTabsMoveParams(
        workspaceId: workspaceId, paneId: paneId, index: UInt32(max(0, index)),
        idempotencyKey: UUID().uuidString)
    ).decoded(EditorTabsListResult.self)
    editorWorkspace.tabs[workspaceId] = result.tabs
  }

  /// Lets a platform surface drain the shared editor writer before it unmounts.
  public func flushEditorEdits(fileId: String) async {
    await settleEditorWrites(fileId)
  }

  func applyEditorWorkspaceEvent(_ eventName: EventName, _ params: JsonValue) throws {
    switch eventName {
    case .editorTabsUpdated:
      let event = try params.decoded(EditorTabsUpdatedEvent.self)
      editorWorkspace.tabs[event.workspaceId] = event.tabs
    case .projectEntryRenamed:
      let event = try params.decoded(ProjectEntryRenamedEvent.self)
      for file in event.files { upsertEditor(file) }
    case .projectTreeChanged:
      let event = try params.decoded(ProjectTreeChangedEvent.self)
      Task { await self.refreshProjectDirectories(event.workspaceId, paths: event.paths) }
    default: break
    }
  }
}
