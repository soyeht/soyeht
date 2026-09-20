import SoyehtProtocol
import Testing

@testable import SoyehtClient

@MainActor @Test func paneCreatedCompletesWritingReplicaForASecondClient() throws {
  let client = EngineClient(socket: "/tmp/pane-created-writing.sock")
  let pane = createdWritingPane()
  let document = createdWritingDocument()

  try client.applyPaneCreated(
    PaneCreatedEvent(
      eventId: "1", at: "2026-09-20T12:00:00Z", pane: pane,
      file: document.file, writingDocument: document))

  #expect(client.panes.map(\.paneId) == [pane.paneId])
  #expect(client.editorFiles[document.file.fileId] != nil)
  #expect(client.editorFiles[document.file.fileId]?.viewport == nil)
  #expect(client.writingDocuments[document.file.fileId]?.document.kind == .text)
  #expect(client.writingDocuments[document.file.fileId]?.loadIdentity == "writing-pane:file-1")
}

@MainActor @Test func paneCreatedCompletesEditorReplicaForASecondClient() throws {
  let client = EngineClient(socket: "/tmp/pane-created-editor.sock")
  var pane = createdWritingPane()
  pane.paneId = "editor-pane"
  pane.name = "Editor"
  pane.mode = .editor
  pane.writingKind = nil
  pane.writingSection = nil
  var file = createdWritingDocument().file
  file.paneId = pane.paneId

  try client.applyPaneCreated(
    PaneCreatedEvent(
      eventId: "1", at: "2026-09-20T12:00:00Z", pane: pane, file: file))

  #expect(client.panes.map(\.paneId) == [pane.paneId])
  #expect(client.editorFiles[file.fileId]?.file.path == file.path)
  #expect(client.writingDocuments.isEmpty)
}

@MainActor @Test func paneCreatedWithoutReplicaMetadataKeepsTheLegacyIncompletePane() throws {
  let pane = createdWritingPane()

  let client = EngineClient(socket: "/tmp/pane-created-missing.sock")
  try client.applyPaneCreated(
    PaneCreatedEvent(eventId: "1", at: "2026-09-20T12:00:00Z", pane: pane))

  #expect(client.panes.map(\.paneId) == [pane.paneId])
  #expect(client.editorFiles.isEmpty)
  #expect(client.writingDocuments.isEmpty)
}

@MainActor @Test func paneCreatedRejectsCrossOwnedOrPartialReplicaMetadata() {
  let pane = createdWritingPane()
  let document = createdWritingDocument()

  #expect(throws: ClientFailure.self) {
    try EngineClient(socket: "/tmp/pane-created-partial.sock").applyPaneCreated(
      PaneCreatedEvent(
        eventId: "1", at: "2026-09-20T12:00:00Z", pane: pane,
        file: document.file))
  }

  var wrong = document
  wrong.file.fileId = "another-file"
  #expect(throws: ClientFailure.self) {
    try EngineClient(socket: "/tmp/pane-created-owner.sock").applyPaneCreated(
      PaneCreatedEvent(
        eventId: "1", at: "2026-09-20T12:00:00Z", pane: pane,
        file: document.file, writingDocument: wrong))
  }

  var terminal = pane
  terminal.mode = .terminal
  terminal.writingKind = nil
  terminal.writingSection = nil
  #expect(throws: ClientFailure.self) {
    try EngineClient(socket: "/tmp/pane-created-terminal.sock").applyPaneCreated(
      PaneCreatedEvent(
        eventId: "1", at: "2026-09-20T12:00:00Z", pane: terminal,
        file: document.file))
  }
}

private func createdWritingPane() -> Pane {
  Pane(
    paneId: "writing-pane", workspaceId: "workspace-1", name: "Opening", cwd: "/repo",
    mode: .writing, state: .idle, source: .protocol, confidence: 1,
    phrase: "writing Opening", generation: 1, cols: 80, rows: 24, record: false,
    writingKind: .text, writingSection: .texts)
}

private func createdWritingDocument() -> WritingDocument {
  WritingDocument(
    file: FileDocument(
      fileId: "file-1", paneId: "writing-pane", workspaceId: "workspace-1",
      path: "Writing/Texts/Opening.md", language: .markdown, revision: "1", diskRevision: "1",
      lineCount: 12, byteCount: "240", dirty: false, conflicted: false, watching: true),
    kind: .text, section: .texts, sidecarRevision: "0", font: .reading_serif,
    focusMode: false, fullWidth: false)
}
