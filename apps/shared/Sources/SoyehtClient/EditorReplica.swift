import Foundation
import SoyehtProtocol

public struct EditorFileView: Sendable {
  public var file: FileDocument
  public var viewport: FileReadResult?
  public var changedLines: [UInt32]
  public var lastSavedBy: FileAuthor?
  public var conflict: FileConflictEvent?
  public var diagnostics: [FileDiagnostic]
  public var agentCursors: [String: EditorAgentCursorEvent]
  public var review: EditorReviewRequestedEvent?

  public init(file: FileDocument, viewport: FileReadResult? = nil) {
    self.file = file
    self.viewport = viewport
    changedLines = []
    lastSavedBy = nil
    conflict = nil
    diagnostics = []
    agentCursors = [:]
    review = nil
  }
}

extension EngineClient {
  public var selectedEditorFile: EditorFileView? {
    guard let paneId = selectedPaneId else { return nil }
    return editorFiles.values.first { $0.file.paneId == paneId }
  }

  func applyEditorSnapshot(_ sections: [String: [JsonValue]], panes: [Pane]) throws {
    let livePanes = Set(panes.lazy.filter { $0.mode == .editor }.map(\.paneId))
    var replacement: [String: EditorFileView] = [:]
    for value in sections["files"] ?? [] {
      let file = try value.decoded(FileDocument.self)
      guard livePanes.contains(file.paneId) else { throw ClientFailure.invalidSnapshot }
      replacement[file.fileId] = EditorFileView(file: file)
    }
    editorFiles = replacement
  }

  func applyEditorEvent(_ eventName: EventName, _ params: JsonValue) throws {
    switch eventName {
    case .fileChangedOnDisk:
      let event = try params.decoded(FileChangedOnDiskEvent.self)
      updateEditor(event.fileId) {
        $0.file.diskRevision = event.diskRevision
        $0.changedLines = event.changedLines
      }
      scheduleEditorReload(event.fileId)
    case .fileSaved:
      let event = try params.decoded(FileSavedEvent.self)
      upsertEditor(event.file) {
        $0.lastSavedBy = event.by
        $0.changedLines = []
        $0.conflict = nil
      }
      scheduleEditorReload(event.file.fileId)
    case .fileConflict:
      let event = try params.decoded(FileConflictEvent.self)
      updateEditor(event.fileId) {
        $0.file.revision = event.draftRevision
        $0.file.diskRevision = event.diskRevision
        $0.file.conflicted = true
        $0.conflict = event
      }
    case .lspDiagnostics:
      let event = try params.decoded(LspDiagnosticsEvent.self)
      updateEditor(event.fileId) { $0.diagnostics = event.items }
    case .editorAgentCursor:
      let event = try params.decoded(EditorAgentCursorEvent.self)
      updateEditor(event.fileId) { $0.agentCursors[event.paneId] = event }
    case .editorReviewRequested:
      let event = try params.decoded(EditorReviewRequestedEvent.self)
      updateEditor(event.fileId) { $0.review = event }
    default: break
    }
  }

  func discardEditorPane(_ paneId: String) {
    editorFiles = editorFiles.filter { $0.value.file.paneId != paneId }
  }

  func loadEditorPane(_ paneId: String) async throws {
    guard let view = editorFiles.values.first(where: { $0.file.paneId == paneId }) else {
      throw ClientFailure.invalidSnapshot
    }
    let start = view.viewport?.fromLine ?? 0
    let count = view.viewport.map { $0.toLine - $0.fromLine } ?? 200
    _ = try await readEditorFile(fileId: view.file.fileId, fromLine: start, lineCount: count)
  }

  @discardableResult
  public func openEditorFile(
    workspaceId: String, path: String, line: UInt32? = nil, column: UInt32? = nil
  ) async throws -> FileOpenResult {
    let result = try await mutation(
      "file.open",
      FileOpenParams(
        workspaceId: workspaceId, path: path, line: line, column: column,
        idempotencyKey: UUID().uuidString)
    ).decoded(FileOpenResult.self)
    upsertPane(result.pane)
    acceptEditorRead(result.viewport)
    select(result.pane.paneId)
    return result
  }

  @discardableResult
  public func readEditorFile(
    fileId: String, revision: String? = nil, source: EditorContentSource? = nil,
    versionId: String? = nil, fromLine: UInt32, lineCount: UInt32
  ) async throws -> FileReadResult {
    let result = try await request(
      "file.read",
      .encoded(
        FileReadParams(
          fileId: fileId, revision: revision, source: source, versionId: versionId,
          fromLine: fromLine,
          lineCount: min(lineCount, UInt32(ProtocolLimits.editorViewportLines))))
    )
    .decoded(FileReadResult.self)
    acceptEditorRead(result)
    return result
  }

  @discardableResult
  public func writeEditorFile(
    fileId: String, baseRevision: String, edits: [TextEdit]
  ) async throws -> FileWriteResult {
    guard let actor = connectionActor else { throw ClientFailure.disconnected }
    let result = try await mutation(
      "file.write",
      FileWriteParams(
        fileId: fileId, baseRevision: baseRevision, edits: edits,
        author: FileAuthor(actor: actor, name: "you"), idempotencyKey: UUID().uuidString)
    ).decoded(FileWriteResult.self)
    upsertEditor(result.file) { $0.changedLines = result.changedLines }
    return result
  }

  public func enqueueEditorEdits(fileId: String, edits: [TextEdit]) {
    guard !edits.isEmpty else { return }
    editorWriteQueues[fileId, default: []].append(edits)
    startEditorWriteTask(fileId)
  }

  private func startEditorWriteTask(_ fileId: String) {
    guard editorWriteTasks[fileId] == nil else { return }
    editorWriteTasks[fileId] = Task { [weak self] in
      await self?.drainEditorEdits(fileId)
    }
  }

  @discardableResult
  public func watchEditorFile(_ fileId: String, watch: Bool) async throws -> FileWatchResult {
    let result = try await mutation(
      "file.watch",
      FileWatchParams(fileId: fileId, watch: watch, idempotencyKey: UUID().uuidString)
    ).decoded(FileWatchResult.self)
    updateEditor(fileId) { $0.file.watching = result.watching }
    return result
  }

  public func editorHistory(
    _ fileId: String, cursor: String? = nil, limit: UInt32 = 50
  ) async throws -> FileHistoryResult {
    try await request(
      "file.history",
      .encoded(FileHistoryParams(fileId: fileId, cursor: cursor, limit: limit))
    )
    .decoded(FileHistoryResult.self)
  }

  @discardableResult
  public func revertEditorFile(
    _ fileId: String, versionId: String, baseRevision: String
  ) async throws -> FileRevertResult {
    let result = try await mutation(
      "file.revert",
      FileRevertParams(
        fileId: fileId, versionId: versionId, baseRevision: baseRevision,
        idempotencyKey: UUID().uuidString)
    ).decoded(FileRevertResult.self)
    upsertEditor(result.file)
    try await refreshEditorFile(fileId)
    return result
  }

  public func searchEditorFiles(
    workspaceId: String, scope: FileSearchScope = .content, query: String,
    replacement: String? = nil, caseSensitive: Bool = false,
    limit: UInt32 = UInt32(ProtocolLimits.editorSearchResults)
  ) async throws -> FileSearchResult {
    try await mutation(
      "file.search",
      FileSearchParams(
        workspaceId: workspaceId, scope: scope, query: query, replacement: replacement,
        caseSensitive: caseSensitive,
        limit: min(limit, UInt32(ProtocolLimits.editorSearchResults)),
        idempotencyKey: UUID().uuidString)
    )
    .decoded(FileSearchResult.self)
  }

  private func refreshEditorFile(_ fileId: String) async throws {
    guard let viewport = editorFiles[fileId]?.viewport else { return }
    _ = try await readEditorFile(
      fileId: fileId, fromLine: viewport.fromLine,
      lineCount: max(1, viewport.toLine - viewport.fromLine))
  }

  private func drainEditorEdits(_ fileId: String) async {
    defer {
      editorWriteTasks[fileId] = nil
      if editorWriteQueues[fileId]?.isEmpty == false { startEditorWriteTask(fileId) }
    }
    while !Task.isCancelled, let edits = editorWriteQueues[fileId]?.first {
      guard let revision = editorFiles[fileId]?.file.revision else {
        editorWriteQueues[fileId] = nil
        return
      }
      do {
        _ = try await writeEditorFile(fileId: fileId, baseRevision: revision, edits: edits)
        editorWriteQueues[fileId]?.removeFirst()
      } catch {
        editorWriteQueues[fileId] = nil
        problem = error.localizedDescription
        try? await refreshEditorFile(fileId)
        return
      }
    }
    editorWriteQueues[fileId] = nil
    try? await refreshEditorFile(fileId)
  }

  private func scheduleEditorReload(_ fileId: String) {
    guard connected, editorFiles[fileId]?.viewport != nil else { return }
    perform { try await self.refreshEditorFile(fileId) }
  }

  private func acceptEditorRead(_ result: FileReadResult) {
    upsertEditor(result.file) { $0.viewport = result }
  }

  private func upsertEditor(
    _ file: FileDocument, update: (inout EditorFileView) -> Void = { _ in }
  ) {
    var view = editorFiles[file.fileId] ?? EditorFileView(file: file)
    view.file = file
    update(&view)
    editorFiles[file.fileId] = view
  }

  private func updateEditor(_ fileId: String, update: (inout EditorFileView) -> Void) {
    guard var view = editorFiles[fileId] else { return }
    update(&view)
    editorFiles[fileId] = view
  }
}
