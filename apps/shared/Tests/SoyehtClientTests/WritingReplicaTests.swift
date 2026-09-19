import SoyehtProtocol
import Testing

@testable import SoyehtClient

@MainActor @Test func writingSnapshotRequiresOneMatchingLiveOwnedFile() throws {
  let client = EngineClient(socket: "/tmp/writing-replica.sock")
  let pane = writingPane()
  let document = writingDocument()
  try client.applyEditorSnapshot(["files": [try .encoded(document.file)]], panes: [pane])
  try client.applyWritingSnapshot(["writing": [try .encoded(document)]], panes: [pane])
  client.select(pane.paneId)

  #expect(client.selectedWritingDocument?.document.file.fileId == document.file.fileId)
  #expect(client.selectedWritingFile?.file.path == "Texts/Opening.md")

  var wrong = document
  wrong.kind = .screenplay
  #expect(throws: ClientFailure.self) {
    try client.applyWritingSnapshot(["writing": [try .encoded(wrong)]], panes: [pane])
  }
}

@MainActor @Test func writingEventAppliesOnlyPublishedRevisionsAndClosingDropsTheReplica() throws {
  let client = EngineClient(socket: "/tmp/writing-event.sock")
  let pane = writingPane()
  let document = writingDocument()
  client.upsertPane(pane)
  client.editorFiles[document.file.fileId] = EditorFileView(file: document.file)
  client.writingDocuments[document.file.fileId] = WritingDocumentView(document: document)

  try client.applyWritingEvent(
    .encoded(
      WritingChangedEvent(
        eventId: "1", at: "2026-09-14T12:00:00Z", fileId: document.file.fileId,
        paneId: pane.paneId, revision: "2", sidecarRevision: "4")))

  #expect(client.writingDocuments[document.file.fileId]?.document.file.revision == "2")
  #expect(client.writingDocuments[document.file.fileId]?.document.sidecarRevision == "4")
  #expect(client.editorFiles[document.file.fileId]?.file.revision == "2")

  var wrongPane = pane
  wrongPane.paneId = "another-pane"
  #expect(throws: ClientFailure.self) {
    try client.applyWritingEvent(
      .encoded(
        WritingChangedEvent(
          eventId: "2", at: "2026-09-14T12:00:01Z", fileId: document.file.fileId,
          paneId: wrongPane.paneId, revision: "3", sidecarRevision: "5")))
  }

  client.discardWritingPane(pane.paneId)
  #expect(client.writingDocuments.isEmpty)
}

@MainActor @Test func editorWritesAdvanceTheWritingRevisionWithoutBlankingItsStructure() {
  let client = EngineClient(socket: "/tmp/writing-editor-sync.sock")
  var document = writingDocument()
  client.editorFiles[document.file.fileId] = EditorFileView(file: document.file)
  client.writingDocuments[document.file.fileId] = WritingDocumentView(
    document: document, structure: writingStructure(document))
  let loadIdentity = client.writingDocuments[document.file.fileId]?.loadIdentity

  document.file.revision = "2"
  document.file.dirty = true
  client.upsertEditor(document.file)

  for revision in 3...402 {
    document.file.revision = String(revision)
    client.upsertEditor(document.file)
  }

  #expect(client.writingDocuments[document.file.fileId]?.document.file.revision == "402")
  #expect(client.writingDocuments[document.file.fileId]?.document.file.dirty == true)
  #expect(client.writingDocuments[document.file.fileId]?.structure != nil)
  #expect(client.writingDocuments[document.file.fileId]?.loadIdentity == loadIdentity)
}

private func writingPane() -> Pane {
  Pane(
    paneId: "writing-pane", workspaceId: "workspace-1", name: "Opening", cwd: "/repo",
    mode: .writing, state: .idle, source: .protocol, confidence: 1,
    phrase: "writing Opening", generation: 1, cols: 80, rows: 24, record: false,
    writingKind: .text, writingSection: .texts)
}

private func writingDocument() -> WritingDocument {
  WritingDocument(
    file: FileDocument(
      fileId: "file-1", paneId: "writing-pane", workspaceId: "workspace-1",
      path: "Texts/Opening.md", language: .markdown, revision: "1", diskRevision: "1",
      lineCount: 12, byteCount: "240", dirty: false, conflicted: false, watching: true),
    kind: .text, section: .texts, sidecarRevision: "0", font: .reading_serif,
    focusMode: false, fullWidth: false)
}

private func writingStructure(_ document: WritingDocument) -> WritingStructureResult {
  WritingStructureResult(
    document: document, slides: [], scenes: [], elements: [], variants: [], beats: [],
    truncated: false)
}
