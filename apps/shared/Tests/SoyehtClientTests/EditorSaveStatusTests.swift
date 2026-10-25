import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func document(dirty: Bool = false) -> FileDocument {
  FileDocument(
    fileId: "file-1", paneId: "editor-pane", workspaceId: "workspace-1",
    path: "Sources/main.swift", language: .swift, revision: "1", diskRevision: "1",
    lineCount: 8, byteCount: "120", dirty: dirty, conflicted: false, watching: true)
}

private func page(dirty: Bool = false, failure: EditorSaveFailure? = nil) -> FileReadResult {
  FileReadResult(
    file: document(dirty: dirty), source: .buffer, fromLine: 0, toLine: 0, lines: [],
    saveFailure: failure)
}

private func text(key: String) -> String? {
  switch key {
  case "editorSaveMissing": ClientPolicy.editorSaveMissing
  case "editorSaveDenied": ClientPolicy.editorSaveDenied
  case "editorSaveFull": ClientPolicy.editorSaveFull
  case "editorSaveOther": ClientPolicy.editorSaveOther
  default: nil
  }
}

@Test func whatThePaneSaysOfAFailedSaveFollowsTheEditorVector() throws {
  let vector = try ContractVector.load("editor")
  for item in ContractVector.cases(vector["saveStatus"], "cases") {
    let label = item["name"].string ?? ""
    var status = EditorSaveStatus()
    for step in item["steps"].array ?? [] {
      if let told = step["told"].string {
        let reason = try #require(EditorSaveFailure(rawValue: told), "\(label)")
        status.told(reason)
      } else if step["saved"].bool != nil {
        status.saved()
      } else if step["read"] != .null {
        let failure = step["read"]["saveFailure"].string.flatMap(EditorSaveFailure.init)
        status.read(page(dirty: step["read"]["dirty"].bool == true, failure: failure))
      } else {
        #expect(step["typed"].bool != nil, "\(label): unknown step")
      }
    }
    #expect(status.text == item["shows"].string.flatMap(text(key:)), "\(label)")
  }
}

@Test func everyReasonHasItsOwnText() {
  let texts = EditorSaveFailure.allCases.map(EditorSaveStatus.text(of:))
  #expect(Set(texts).count == EditorSaveFailure.allCases.count)
  #expect(texts.allSatisfy { $0.hasPrefix("Couldn't save") })
}

@MainActor @Test func theEngineTellingASaveFailedShowsInThePaneUntilTheFileIsSaved() throws {
  let client = EngineClient(socket: "/tmp/editor-save-status.sock")
  var file = document(dirty: true)
  client.editorFiles[file.fileId] = EditorFileView(file: file)

  try client.applyEditorEvent(
    .fileSaveFailed, .encoded(FileSaveFailedEvent(fileId: file.fileId, reason: .missing)))
  #expect(client.editorFiles[file.fileId]?.saveStatus.text == ClientPolicy.editorSaveMissing)

  // A read of the file the engine is saving again ends it; a read that still fails keeps it.
  client.acceptEditorRead(page(dirty: true, failure: .denied))
  #expect(client.editorFiles[file.fileId]?.saveStatus.text == ClientPolicy.editorSaveDenied)

  file.dirty = false
  try client.applyEditorEvent(
    .fileSaved,
    .encoded(
      FileSavedEvent(
        eventId: "2", at: "2026-09-12T20:00:01Z", file: file,
        by: FileAuthor(
          actor: Actor(role: .human_local, client: "mac", deviceId: nil, paneId: nil),
          name: "you"))))
  #expect(client.editorFiles[file.fileId]?.saveStatus.text == nil)
}

@MainActor @Test func aFailureToldOfAFileThePaneDoesNotHoldIsIgnored() throws {
  let client = EngineClient(socket: "/tmp/editor-save-status-other.sock")
  try client.applyEditorEvent(
    .fileSaveFailed, .encoded(FileSaveFailedEvent(fileId: "other", reason: .full)))
  #expect(client.editorFiles.isEmpty)
}
