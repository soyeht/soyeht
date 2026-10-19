import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func refusal(kind: String) -> ClientFailure {
  ClientFailure.remote(kind, "engine text that the pane must not repeat")
}

@MainActor private func client() -> EngineClient {
  EngineClient(socket: "/tmp/editor-open-refusal.sock")
}

@Test func whereAFailedOpenIsShownFollowsTheEditorVector() throws {
  let vector = try ContractVector.load("editor")
  for item in ContractVector.cases(vector["openFailure"], "cases") {
    let label = item["name"].string ?? ""
    let kind = try #require(item["kind"].string, "\(label)")
    let found = EditorOpenRefusal(error: refusal(kind: kind), path: "blob.bin")
    switch item["placement"].string {
    case "inline":
      let text: String? =
        switch item["text"].string {
        case "editorFileNotText": ClientPolicy.editorFileNotText
        case "editorFileTooLarge": ClientPolicy.editorFileTooLarge
        default: nil
        }
      #expect(found?.text == text && text != nil, "\(label)")
    default:
      #expect(found == nil, "\(label)")
    }
  }
}

@Test func aFailureThatIsNotARemoteRefusalIsNeverAPaneState() {
  #expect(EditorOpenRefusal(error: ClientFailure.disconnected, path: "a") == nil)
  #expect(EditorOpenRefusal(error: ClientFailure.timeout, path: "a") == nil)
}

@MainActor @Test func aBinaryFileIsAStateOfTheEditorAndOpensNoDialog() {
  let client = client()

  client.reportEditorOpenFailure(
    refusal(kind: ErrorKind.file_not_text.rawValue), workspaceId: "w1", path: "blob.bin")

  #expect(client.problem == nil)
  #expect(
    client.editorOpenRefusal(in: "w1")
      == EditorOpenRefusal(
        error: refusal(kind: ErrorKind.file_not_text.rawValue), path: "blob.bin"))
  #expect(client.editorOpenRefusal(in: "w1")?.text == ClientPolicy.editorFileNotText)
  #expect(client.editorOpenRefusal(in: "w2") == nil)
}

@MainActor @Test func anyOtherOpenFailureIsTheDialogAndLeavesThePaneAlone() {
  let client = client()

  client.reportEditorOpenFailure(
    refusal(kind: ErrorKind.not_found.rawValue), workspaceId: "w1", path: "gone.txt")

  #expect(client.problem == "engine text that the pane must not repeat")
  #expect(client.editorOpenRefusal(in: "w1") == nil)
}

@MainActor @Test func aRefusalGivesWayWhenATabIsChosen() throws {
  let client = client()
  let pane = Pane(
    paneId: "pane-1", workspaceId: "w1", name: "a.py", cwd: "/repo", mode: .editor,
    state: .idle, source: .protocol, confidence: 1, phrase: "editing a.py", generation: 1,
    cols: 80, rows: 24, record: false)
  try client.applyPaneCreated(
    PaneCreatedEvent(eventId: "1", at: "2026-10-18T12:00:00Z", pane: pane, file: nil))
  client.reportEditorOpenFailure(
    refusal(kind: ErrorKind.file_too_large.rawValue), workspaceId: "w1", path: "big.bin")
  #expect(client.editorOpenRefusal(in: "w1")?.text == ClientPolicy.editorFileTooLarge)

  client.activateEditorTab("pane-1")

  #expect(client.editorOpenRefusal(in: "w1") == nil)
}

private func editorPane(_ id: String, workspace: String = "w1") -> Pane {
  Pane(
    paneId: id, workspaceId: workspace, name: "\(id).py", cwd: "/repo", mode: .editor,
    state: .idle, source: .protocol, confidence: 1, phrase: "editing", generation: 1,
    cols: 80, rows: 24, record: false)
}

private func document(for pane: Pane) -> FileDocument {
  FileDocument(
    fileId: "file-\(pane.paneId)", paneId: pane.paneId, workspaceId: pane.workspaceId,
    path: "\(pane.paneId).py", language: .python, revision: "1", diskRevision: "1",
    lineCount: 1, byteCount: "1", dirty: false, conflicted: false, watching: true)
}

@MainActor private func opened(_ pane: Pane, on client: EngineClient) throws {
  try client.applyPaneCreated(
    PaneCreatedEvent(
      eventId: "1", at: "2026-10-18T12:00:00Z", pane: pane, file: document(for: pane)))
}

@MainActor private func refusing(_ client: EngineClient) {
  client.reportEditorOpenFailure(
    refusal(kind: ErrorKind.file_not_text.rawValue), workspaceId: "w1", path: "blob.bin")
  #expect(client.editorOpenRefusal(in: "w1") != nil)
}

@MainActor @Test func aLaterOpenThatFailsForAnotherReasonDropsTheRefusal() {
  let client = client()
  refusing(client)

  client.reportEditorOpenFailure(
    refusal(kind: ErrorKind.not_found.rawValue), workspaceId: "w1", path: "gone.txt")

  #expect(client.editorOpenRefusal(in: "w1") == nil)
  #expect(client.problem != nil)
}

@MainActor @Test func anEditorPaneThatArrivesByEventShowsItsFileNotTheRefusal() throws {
  let client = client()
  refusing(client)

  try opened(editorPane("pane-2"), on: client)

  #expect(client.editorOpenRefusal(in: "w1") == nil)
}

@MainActor @Test func aPaneOfAnotherWorkspaceLeavesTheRefusalAlone() throws {
  let client = client()
  refusing(client)

  try opened(editorPane("pane-9", workspace: "w2"), on: client)

  #expect(client.editorOpenRefusal(in: "w1") != nil)
}

@MainActor @Test func closingTheLastEditorTabDropsTheRefusal() throws {
  let client = client()
  try opened(editorPane("pane-1"), on: client)
  refusing(client)

  client.removePane("pane-1")

  #expect(client.editorOpenRefusal(in: "w1") == nil)
}

@MainActor @Test func closingOneOfTwoEditorTabsKeepsTheRefusal() throws {
  let client = client()
  try opened(editorPane("pane-1"), on: client)
  try opened(editorPane("pane-2"), on: client)
  refusing(client)

  client.removePane("pane-1")

  #expect(client.editorOpenRefusal(in: "w1") != nil)
}

@MainActor @Test func closingTheWorkspaceDropsTheRefusal() {
  let client = client()
  refusing(client)

  client.applyWorkspaceClosed(.object(["workspaceId": .string("w1")]))

  #expect(client.editorOpenRefusal(in: "w1") == nil)
}
