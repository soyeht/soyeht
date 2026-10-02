import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func closingATabActivatesThePreviousTabThenANeighbour() {
  let tabs = ["a", "b", "c", "d"]
  #expect(EditorTabPolicy.successor(closing: "c", tabs: tabs, history: ["c", "a", "d"]) == "a")
  #expect(EditorTabPolicy.successor(closing: "b", tabs: tabs, history: []) == "c")
  #expect(EditorTabPolicy.successor(closing: "d", tabs: tabs, history: ["d", "x"]) == "c")
  #expect(EditorTabPolicy.successor(closing: "a", tabs: ["a"], history: []) == nil)
}

@Test func tabCyclingWrapsBothWays() {
  let tabs = ["a", "b", "c"]
  #expect(EditorTabPolicy.cycled(from: "c", tabs: tabs, forward: true) == "a")
  #expect(EditorTabPolicy.cycled(from: "a", tabs: tabs, forward: false) == "c")
  #expect(EditorTabPolicy.cycled(from: "b", tabs: tabs, forward: true) == "c")
  #expect(EditorTabPolicy.cycled(from: nil, tabs: tabs, forward: false) == "c")
  #expect(EditorTabPolicy.cycled(from: "a", tabs: [], forward: true) == nil)
}

@Test func dragReorderAndActivationFollowTheEngineOrder() {
  let tabs = ["a", "b", "c"]
  #expect(EditorTabPolicy.moved(tabs, paneId: "c", to: 0) == ["c", "a", "b"])
  #expect(EditorTabPolicy.moved(tabs, paneId: "a", to: 9) == ["b", "c", "a"])
  #expect(EditorTabPolicy.dropIndex(moving: "a", onto: "c", tabs: tabs) == 2)
  #expect(EditorTabPolicy.active(selected: "terminal", tabs: tabs, history: ["x", "b"]) == "b")
  #expect(EditorTabPolicy.active(selected: "c", tabs: tabs, history: ["b"]) == "c")
  #expect(EditorTabPolicy.recorded("b", in: ["a", "b", "c"]) == ["b", "a", "c"])
}

@Test func aPreviewEndsWithTheFirstEditAndOnlyAConflictAsksBeforeClosing() {
  let tab = EditorTab(
    paneId: "pane-a", fileId: "file-a", path: "a.rs", preview: true, previewRevision: "3")
  #expect(EditorTabPolicy.isPreview(tab, file: tabFile("a", revision: "3")))
  #expect(!EditorTabPolicy.isPreview(tab, file: tabFile("a", revision: "4")))
  #expect(!EditorTabPolicy.closeNeedsConfirmation(tabFile("a", revision: "3")))
  #expect(EditorTabPolicy.closeNeedsConfirmation(tabFile("a", revision: "3", conflicted: true)))
}

@MainActor @Test func theEditorDestinationShowsOnlyTheActiveTabInEngineOrder() throws {
  let client = EngineClient(socket: "/tmp/editor-tabs.sock")
  for name in ["a", "b", "c"] {
    client.upsertPane(tabPane(name))
    client.upsertEditor(tabFile(name, revision: "1"))
  }
  client.upsertPane(tabPane("shell", mode: .terminal))
  #expect(client.editorTabs(in: "workspace-1").map(\.paneId) == ["pane-a", "pane-b", "pane-c"])

  let ordered = EditorTabsUpdatedEvent(
    eventId: "1", at: "2026-10-01T12:00:00Z", workspaceId: "workspace-1",
    tabs: ["c", "a"].map {
      EditorTab(paneId: "pane-\($0)", fileId: "file-\($0)", path: "\($0).rs", preview: false)
    })
  try client.applyEditorEvent(.editorTabsUpdated, .encoded(ordered))
  #expect(client.editorTabs(in: "workspace-1").map(\.paneId) == ["pane-c", "pane-a", "pane-b"])

  client.activateEditorTab("pane-a")
  let shown = client.paneGridPanes(client.panes, mode: .editor)
  #expect(shown.map(\.paneId) == ["pane-a"])
  #expect(client.paneGridPanes(client.panes, mode: .terminal).map(\.paneId) == ["pane-shell"])
  // The agents grid shows the editor panes as one area too.
  #expect(
    client.paneGridPanes(client.panes, mode: nil).map(\.paneId) == ["pane-a", "pane-shell"])

  client.select("pane-shell")
  #expect(client.activeEditorPaneId(in: "workspace-1") == "pane-a")
  client.cycleEditorTab(in: "workspace-1", forward: true)
  #expect(client.selectedPaneId == "pane-b")
  client.cycleEditorTab(in: "workspace-1", forward: true)
  #expect(client.selectedPaneId == "pane-c")
}

@MainActor @Test func aRenameEventMovesTheOpenFile() throws {
  let client = EngineClient(socket: "/tmp/editor-rename.sock")
  client.upsertPane(tabPane("a"))
  client.upsertEditor(tabFile("a", revision: "1"))
  var moved = tabFile("a", revision: "2")
  moved.path = "lib/a.rs"
  try client.applyEditorEvent(
    .projectEntryRenamed,
    .encoded(
      ProjectEntryRenamedEvent(
        eventId: "2", at: "2026-10-01T12:00:00Z", workspaceId: "workspace-1", from: "a.rs",
        to: "lib/a.rs", files: [moved])))
  #expect(client.editorFiles["file-a"]?.file.path == "lib/a.rs")
}

private func tabPane(_ name: String, mode: PaneMode = .editor) -> Pane {
  Pane(
    paneId: "pane-\(name)", workspaceId: "workspace-1", name: name, cwd: "/repo", mode: mode,
    state: .idle, source: .protocol, confidence: 1, phrase: name, generation: 1, cols: 80,
    rows: 24, record: false)
}

private func tabFile(_ name: String, revision: String, conflicted: Bool = false) -> FileDocument {
  FileDocument(
    fileId: "file-\(name)", paneId: "pane-\(name)", workspaceId: "workspace-1",
    path: "\(name).rs", language: .rust, revision: revision, diskRevision: "1", lineCount: 1,
    byteCount: "1", dirty: conflicted, conflicted: conflicted, watching: true)
}
