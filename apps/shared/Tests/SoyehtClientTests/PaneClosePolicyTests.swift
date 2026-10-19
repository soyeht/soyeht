import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func pane(_ id: String, mode: PaneMode, workspace: String = "w") -> Pane {
  Pane(
    paneId: id, workspaceId: workspace, name: id, cwd: "/repo", mode: mode, state: .idle,
    source: .protocol, confidence: 1, phrase: "idle", generation: 1, cols: 80, rows: 24,
    record: false)
}

private func file(path: String, dirty: Bool = false, conflicted: Bool = false) -> FileDocument {
  FileDocument(
    fileId: "f", paneId: "p", workspaceId: "w", path: path, language: .plain_text, revision: "1",
    diskRevision: "1", lineCount: 1, byteCount: "1", dirty: dirty, conflicted: conflicted,
    watching: true)
}

private func workspace(_ name: String) -> Workspace {
  Workspace(workspaceId: "w", name: name, path: "/repo", layout: .object([:]))
}

@Test func aPaneThatRunsAProcessAlwaysAsksFirst() {
  for mode in [PaneMode.terminal, .agent, .web] {
    #expect(
      PaneClosePolicy.confirmation(for: pane("p", mode: mode), file: nil) == .runningPane)
  }
  #expect(CloseConfirmation.runningPane.title == "Close pane?")
  #expect(CloseConfirmation.runningPane.message == "The process in this pane will be terminated.")
}

@Test func aDocumentAsksOnlyWhileItHoldsUnsavedChanges() {
  let writing = pane("p", mode: .writing)
  #expect(
    PaneClosePolicy.confirmation(for: writing, file: file(path: "/repo/texts/essay.md")) == nil)
  let asked = PaneClosePolicy.confirmation(
    for: writing, file: file(path: "/repo/texts/essay.md", dirty: true))
  #expect(asked?.title == "Close essay.md?")
  #expect(asked?.message == "Unsaved changes will be lost.")
}

@Test func anEditorTabAsksOnlyWhenItsDraftConflictsWithTheDisk() {
  let editor = pane("p", mode: .editor)
  #expect(
    PaneClosePolicy.confirmation(for: editor, file: file(path: "/repo/a.rs", dirty: true)) == nil)
  #expect(PaneClosePolicy.confirmation(for: editor, file: nil) == nil)
  let asked = PaneClosePolicy.confirmation(
    for: editor, file: file(path: "/repo/a.rs", conflicted: true))
  #expect(asked?.title == "Close a.rs?")
}

@Test func aWorkspaceAsksOnlyWhenItHoldsPanes() {
  #expect(PaneClosePolicy.confirmation(for: workspace("repo"), paneCount: 0) == nil)
  let one = PaneClosePolicy.confirmation(for: workspace("repo"), paneCount: 1)
  #expect(one?.title == "Close repo?")
  #expect(one?.message.contains("its pane") == true)
  #expect(one?.confirmTitle == "Close workspace")
  let three = PaneClosePolicy.confirmation(for: workspace("repo"), paneCount: 3)
  #expect(three?.message.contains("its 3 panes") == true)
}

@MainActor @Test func theClientAsksAboutAPaneItHoldsAndAboutItsWorkspace() throws {
  let client = EngineClient(socket: "/tmp/pane-close-policy.sock")
  client.upsertWorkspace(workspace("repo"))
  try client.applyPaneCreated(
    PaneCreatedEvent(eventId: "1", at: "2026-10-19T12:00:00Z", pane: pane("t", mode: .terminal)))
  try client.applyPaneCreated(
    PaneCreatedEvent(eventId: "2", at: "2026-10-19T12:00:01Z", pane: pane("a", mode: .agent)))

  #expect(client.closeConfirmation(forPane: "t") == .runningPane)
  #expect(client.closeConfirmation(forPane: "missing") == nil)
  #expect(client.closeConfirmation(forWorkspace: "w")?.message.contains("its 2 panes") == true)
  #expect(client.closeConfirmation(forWorkspace: "missing") == nil)
}

@MainActor @Test func aWorkspaceTheEngineRefusesToCloseStaysAndTheClientSaysSo() async {
  let client = EngineClient(socket: "/tmp/pane-close-workspace.sock")
  client.upsertWorkspace(workspace("repo"))
  var methods: [RequestMethod] = []
  client.requestStub = { method, _ in
    methods.append(method)
    throw ClientFailure.remote("internal_error", "Engine failed")
  }

  let closed = await client.closeWorkspaceReportingFailure("w")

  #expect(!closed)
  #expect(methods == [.workspaceClose])
  #expect(client.workspaces.map(\.workspaceId) == ["w"])
  #expect(client.problem == "Couldn't close the workspace. It is still available.")
}

@MainActor @Test func aWorkspaceTheEngineClosesReportsNoProblem() async {
  let client = EngineClient(socket: "/tmp/pane-close-workspace-ok.sock")
  client.requestStub = { _, _ in .object([:]) }

  let closed = await client.closeWorkspaceReportingFailure("w")

  #expect(closed)
  #expect(client.problem == nil)
}
