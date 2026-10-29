import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func pane(_ id: String, mode: PaneMode, workspace: String = "w") -> Pane {
  PaneLayoutFixture.pane(id, workspace: workspace, mode: mode)
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

/// The cases of `contracts/vectors/pane-close.json`, which the desktop client passes as well.
@Test func aPaneAsksAsTheVectorSays() throws {
  let vector = try ContractVector.load("pane-close")
  for item in ContractVector.cases(vector, "panes") {
    let name = item["name"].string ?? ""
    let modeName = try #require(item["mode"].string)
    let mode = try #require(PaneMode(rawValue: modeName), "\(name)")
    let asked = PaneClosePolicy.confirmation(for: pane("p", mode: mode), file: nil)
    let confirm = item["confirm"]
    if case .null = confirm {
      #expect(asked == nil, "\(name)")
    } else {
      let title = try #require(confirm["title"].string)
      let message = try #require(confirm["message"].string)
      let confirmTitle = try #require(confirm["confirmTitle"].string)
      #expect(
        asked == CloseConfirmation(title: title, message: message, confirmTitle: confirmTitle),
        "\(name)")
    }
  }
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

/// The `workspaces` cases of `contracts/vectors/pane-close.json`.
@Test func aWorkspaceAsksAsTheVectorSays() throws {
  let vector = try ContractVector.load("pane-close")
  let place = try #require(vector["places"]["mac"].string)
  for item in ContractVector.cases(vector, "workspaces") {
    let name = item["name"].string ?? ""
    let workspaceName = try #require(item["workspaceName"].string)
    let paneCount = try #require(item["paneCount"].int)
    let asked = PaneClosePolicy.confirmation(for: workspace(workspaceName), paneCount: paneCount)
    let confirm = item["confirm"]
    if case .null = confirm {
      #expect(asked == nil, "\(name)")
    } else {
      #expect(
        asked
          == CloseConfirmation(
            title: try #require(confirm["title"].string),
            message: try #require(confirm["message"].string)
              .replacingOccurrences(of: "{place}", with: place),
            confirmTitle: try #require(confirm["confirmTitle"].string)),
        "\(name)")
    }
  }
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

private func textEdit() -> TextEdit {
  TextEdit(
    range: TextRange(
      start: TextPosition(line: 0, column: 0), end: TextPosition(line: 0, column: 0)),
    text: "x")
}

/// A closed connection cannot write, so a write that was attempted shows as `problem`: it says
/// whether the edits were sent before the engine was asked to close the workspace.
@MainActor private func problemWhenTheWorkspaceCloses(
  editedFileIn editedWorkspace: String
) async -> String? {
  let client = EngineClient(socket: "/tmp/pane-close-flush-\(editedWorkspace).sock")
  client.upsertWorkspace(workspace("repo"))
  var edited = file(path: "/repo/texts/essay.md")
  edited.workspaceId = editedWorkspace
  client.editorFiles[edited.fileId] = EditorFileView(file: edited)
  var problemAtClose: String?
  client.requestStub = { method, _ in
    if method == .workspaceClose { problemAtClose = client.problem }
    return .object([:])
  }
  client.enqueueEditorEdits(fileId: edited.fileId, edits: [textEdit()])
  _ = await client.closeWorkspaceReportingFailure("w")
  return problemAtClose
}

@MainActor @Test func closingAWorkspaceSendsWhatWasTypedInItsDocumentsFirst() async {
  #expect(await problemWhenTheWorkspaceCloses(editedFileIn: "w") != nil)
}

@MainActor @Test func closingAWorkspaceLeavesTheDocumentsOfOthersAlone() async {
  #expect(await problemWhenTheWorkspaceCloses(editedFileIn: "other") == nil)
}

/// Plays the close of the pane `p` of a client whose file `f` holds a typed edit, and returns the
/// requests the engine saw and, for the close, whether the edit had been tried by then (a closed
/// connection cannot write, so a write that was attempted shows as `problem`).
@MainActor private func closing(
  _ mode: PaneMode, typing: Bool = true, extraEditorPane: Bool = false,
  engineAnswersEdits: Bool = false
) async throws -> (methods: [RequestMethod], editSentFirst: Bool, client: EngineClient) {
  let client = EngineClient(socket: "/tmp/pane-close-saving-\(mode.rawValue).sock")
  client.upsertWorkspace(workspace("repo"))
  try client.applyPaneCreated(
    PaneCreatedEvent(eventId: "1", at: "2026-10-19T12:00:00Z", pane: pane("p", mode: mode)))
  if extraEditorPane {
    try client.applyPaneCreated(
      PaneCreatedEvent(eventId: "2", at: "2026-10-19T12:00:01Z", pane: pane("q", mode: mode)))
    var other = file(path: "/repo/b.swift")
    other.fileId = "g"
    other.paneId = "q"
    client.editorFiles["g"] = EditorFileView(file: other)
  }
  client.editorFiles["f"] = EditorFileView(file: file(path: "/repo/texts/essay.md"))
  if engineAnswersEdits { client.connectionActor = Actor(role: .human_local, client: "test") }
  let written = FileWriteResult(
    file: file(path: "/repo/texts/essay.md", dirty: true), changedLines: [0])
  var methods: [RequestMethod] = []
  var editSentFirst = false
  client.requestStub = { method, _ in
    if method == .paneClose { editSentFirst = client.problem != nil }
    methods.append(method)
    return method == .fileWrite ? try JsonValue.encoded(written) : .object([:])
  }
  if typing { client.enqueueEditorEdits(fileId: "f", edits: [textEdit()]) }
  await client.closePaneSavingEdits("p")
  for _ in 0..<100 where !methods.contains(.paneClose) { await Task.yield() }
  return (methods, editSentFirst, client)
}

@MainActor @Test func closingAWritingPaneSendsWhatWasTypedBeforeTheClose() async throws {
  let typed = try await closing(.writing)
  #expect(typed.methods == [.paneClose])
  #expect(typed.editSentFirst)
  #expect(typed.client.problem != nil)

  let untouched = try await closing(.writing, typing: false)
  #expect(untouched.methods == [.paneClose])
  #expect(!untouched.editSentFirst)
}

/// The engine takes the edit: it is the request before the close, and nothing is left to tell.
@MainActor @Test func closingAPaneWritesTheEditTheEngineAcceptsBeforeTheClose() async throws {
  for mode in [PaneMode.writing, .editor] {
    let closed = try await closing(mode, engineAnswersEdits: true)
    #expect(closed.methods == [.fileWrite, .paneClose], "\(mode)")
    #expect(closed.client.problem == nil, "\(mode)")
  }
}

/// A write the engine refuses is not retried and does not keep the pane: the replica drops the
/// edits and reads the engine's text again, so the pane holds nothing a second close could save.
/// The person is told by `problem`.
@MainActor @Test func aRefusedEditIsReportedAndTheCloseStillGoes() async throws {
  let client = EngineClient(socket: "/tmp/pane-close-refused.sock")
  client.upsertWorkspace(workspace("repo"))
  try client.applyPaneCreated(
    PaneCreatedEvent(eventId: "1", at: "2026-10-19T12:00:00Z", pane: pane("p", mode: .writing)))
  client.editorFiles["f"] = EditorFileView(file: file(path: "/repo/texts/essay.md"))
  client.connectionActor = Actor(role: .human_local, client: "test")
  var methods: [RequestMethod] = []
  client.requestStub = { method, _ in
    methods.append(method)
    if method == .fileWrite { throw ClientFailure.remote("internal_error", "Engine failed") }
    return .object([:])
  }
  client.enqueueEditorEdits(fileId: "f", edits: [textEdit()])
  await client.closePaneSavingEdits("p")
  for _ in 0..<100 where !methods.contains(.paneClose) { await Task.yield() }

  #expect(client.problem != nil)
  #expect(methods.first == .fileWrite)
  #expect(methods.contains(.paneClose))
}

@MainActor @Test func closingAnEditorTabSendsWhatWasTypedAndPassesItsPlaceOn() async throws {
  let closed = try await closing(.editor, extraEditorPane: true)
  #expect(closed.methods == [.paneClose])
  #expect(closed.editSentFirst)
  #expect(closed.client.selectedPaneId == "q")
}

@MainActor @Test func closingAPaneThatRunsAProcessSendsOnlyTheClose() async throws {
  let closed = try await closing(.terminal, typing: false)
  #expect(closed.methods == [.paneClose])
}
