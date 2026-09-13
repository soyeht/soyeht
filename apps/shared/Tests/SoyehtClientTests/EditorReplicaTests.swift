import SoyehtProtocol
import Testing

@testable import SoyehtClient

@MainActor @Test func editorSnapshotReplacesOnlyFilesBoundToLiveEditorPanes() throws {
  let client = EngineClient(socket: "/tmp/editor-replica.sock")
  let pane = editorPane()
  let file = editorFile()
  try client.applyEditorSnapshot(
    ["files": [try .encoded(file)]], panes: [pane])
  client.select(pane.paneId)

  #expect(client.editorFiles.count == 1)
  #expect(client.selectedEditorFile?.file.fileId == file.fileId)

  var orphan = file
  orphan.paneId = "missing-pane"
  #expect(throws: ClientFailure.self) {
    try client.applyEditorSnapshot(["files": [try .encoded(orphan)]], panes: [pane])
  }

  try client.applyEditorSnapshot(["files": []], panes: [pane])
  #expect(client.editorFiles.isEmpty)
}

@MainActor @Test func editorEventsUpdateOnlyEnginePublishedFacts() throws {
  let client = EngineClient(socket: "/tmp/editor-events.sock")
  let file = editorFile()
  client.editorFiles[file.fileId] = EditorFileView(file: file)
  let author = FileAuthor(
    actor: Actor(role: .agent, client: "fixture", paneId: "agent-1"), name: "Worker")

  try client.applyEditorEvent(
    .fileChangedOnDisk,
    .encoded(
      FileChangedOnDiskEvent(
        eventId: "1", at: "2026-09-12T20:00:00Z", fileId: file.fileId,
        diskRevision: "2", changedLines: [3, 4])))
  #expect(client.editorFiles[file.fileId]?.file.diskRevision == "2")
  #expect(client.editorFiles[file.fileId]?.changedLines == [3, 4])

  var saved = file
  saved.revision = "2"
  saved.diskRevision = "2"
  try client.applyEditorEvent(
    .fileSaved,
    .encoded(
      FileSavedEvent(
        eventId: "2", at: "2026-09-12T20:00:01Z", file: saved, by: author)))
  #expect(client.editorFiles[file.fileId]?.file.revision == "2")
  #expect(client.editorFiles[file.fileId]?.lastSavedBy?.name == "Worker")
  #expect(client.editorFiles[file.fileId]?.changedLines.isEmpty == true)

  try client.applyEditorEvent(
    .fileConflict,
    .encoded(
      FileConflictEvent(
        eventId: "3", at: "2026-09-12T20:00:02Z", fileId: file.fileId,
        baseRevision: "1", draftRevision: "2", diskRevision: "3")))
  #expect(client.editorFiles[file.fileId]?.conflict?.diskRevision == "3")
  #expect(client.editorFiles[file.fileId]?.file.conflicted == true)
  #expect(client.editorFiles[file.fileId]?.file.revision == "2")

  let diagnostic = FileDiagnostic(
    diagnosticId: "diagnostic-1", range: textRange(), severity: .warning,
    message: "Check this value", source: "fixture")
  try client.applyEditorEvent(
    .lspDiagnostics,
    .encoded(
      LspDiagnosticsEvent(
        eventId: "4", at: "2026-09-12T20:00:03Z", fileId: file.fileId,
        revision: "2", items: [diagnostic])))
  #expect(client.editorFiles[file.fileId]?.diagnostics.first?.diagnosticId == "diagnostic-1")
  #expect(client.editorFiles[file.fileId]?.diagnosticRevision == "2")

  let stale = FileDiagnostic(
    diagnosticId: "stale", range: textRange(), severity: .error,
    message: "Old position", source: "fixture")
  try client.applyEditorEvent(
    .lspDiagnostics,
    .encoded(
      LspDiagnosticsEvent(
        eventId: "5", at: "2026-09-12T20:00:04Z", fileId: file.fileId,
        revision: "1", items: [stale])))
  #expect(client.editorFiles[file.fileId]?.diagnostics.first?.diagnosticId == "diagnostic-1")

  saved.revision = "3"
  try client.applyEditorEvent(
    .fileSaved,
    .encoded(
      FileSavedEvent(
        eventId: "6", at: "2026-09-12T20:00:05Z", file: saved, by: author)))
  #expect(client.editorFiles[file.fileId]?.diagnostics.isEmpty == true)
  #expect(client.editorFiles[file.fileId]?.diagnosticRevision == nil)

  try client.applyEditorEvent(
    .editorAgentCursor,
    .encoded(
      EditorAgentCursorEvent(
        eventId: "7", at: "2026-09-12T20:00:06Z", fileId: file.fileId,
        paneId: "agent-1", name: "Worker", position: TextPosition(line: 4, column: 2))))
  #expect(client.editorFiles[file.fileId]?.agentCursors["agent-1"]?.position.line == 4)

  try client.applyEditorEvent(
    .editorReviewRequested,
    .encoded(
      EditorReviewRequestedEvent(
        eventId: "8", at: "2026-09-12T20:00:07Z", fileId: file.fileId,
        revision: "2", paneId: "agent-1", hunks: [])))
  #expect(client.editorFiles[file.fileId]?.review == nil)

  let hunk = DiffHunk(
    hunkId: "agent-change", oldStart: 0, oldLines: 1, newStart: 0, newLines: 1,
    lines: [DiffLine(kind: .added, newLine: 0, text: "let value = 2", author: author)])
  try client.applyEditorEvent(
    .editorReviewRequested,
    .encoded(
      EditorReviewRequestedEvent(
        eventId: "9", at: "2026-09-12T20:00:08Z", fileId: file.fileId,
        revision: "2", paneId: "agent-1", hunks: [hunk])))
  #expect(client.editorFiles[file.fileId]?.review == nil)

  saved.revision = "3"
  client.editorFiles[file.fileId] = EditorFileView(file: saved)
  try client.applyEditorEvent(
    .editorReviewRequested,
    .encoded(
      EditorReviewRequestedEvent(
        eventId: "10", at: "2026-09-12T20:00:09Z", fileId: file.fileId,
        revision: "3", paneId: "agent-1", hunks: [hunk])))
  #expect(client.editorFiles[file.fileId]?.review?.hunks.first?.hunkId == "agent-change")

  var next = saved
  next.revision = "4"
  try client.applyEditorEvent(
    .fileSaved,
    .encoded(
      FileSavedEvent(
        eventId: "11", at: "2026-09-12T20:00:10Z", file: next, by: author)))
  #expect(client.editorFiles[file.fileId]?.review == nil)

  client.discardEditorPane(file.paneId)
  #expect(client.editorFiles.isEmpty)
}

private func editorPane() -> Pane {
  Pane(
    paneId: "editor-pane", workspaceId: "workspace-1", name: "main.swift", cwd: "/repo",
    mode: .editor, state: .idle, source: .protocol, confidence: 1, phrase: "editing main.swift",
    generation: 1, cols: 80, rows: 24, record: false)
}

private func editorFile() -> FileDocument {
  FileDocument(
    fileId: "file-1", paneId: "editor-pane", workspaceId: "workspace-1",
    path: "Sources/main.swift", language: .swift, revision: "1", diskRevision: "1",
    lineCount: 8, byteCount: "120", dirty: false, conflicted: false, watching: true)
}

private func textRange() -> TextRange {
  TextRange(
    start: TextPosition(line: 3, column: 1), end: TextPosition(line: 3, column: 5))
}
