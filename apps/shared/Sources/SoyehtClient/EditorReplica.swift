import Foundation
import SoyehtProtocol

public struct EditorFileView: Sendable {
  public var file: FileDocument
  public var viewport: FileReadResult?
  public var changedLines: [UInt32]
  public var lastSavedBy: FileAuthor?
  public var conflict: FileConflictEvent?
  public var diagnostics: [FileDiagnostic]
  public var diagnosticRevision: String?
  public var agentCursors: [String: EditorAgentCursorEvent]
  public var review: EditorReviewRequestedEvent?

  public init(file: FileDocument, viewport: FileReadResult? = nil) {
    self.file = file
    self.viewport = viewport
    changedLines = []
    lastSavedBy = nil
    conflict = nil
    diagnostics = []
    diagnosticRevision = nil
    agentCursors = [:]
    review = nil
  }
}

public struct EditorNavigationTarget: Sendable {
  public let id: String
  public let fileId: String
  public let position: TextPosition
  /// When set, the landing selects `position..<selectionEnd`, such as a match.
  public let selectionEnd: TextPosition?

  public init(
    id: String, fileId: String, position: TextPosition, selectionEnd: TextPosition? = nil
  ) {
    self.id = id
    self.fileId = fileId
    self.position = position
    self.selectionEnd = selectionEnd
  }
}

/// Which end of the engine-owned edit history a replay walks.
public enum EditorHistoryDirection: Sendable {
  case undo
  case redo

  var method: RequestMethod {
    switch self {
    case .undo: .fileUndo
    case .redo: .fileRedo
    }
  }
}

extension EngineClient {
  public var selectedEditorFile: EditorFileView? {
    guard let paneId = selectedPaneId else { return nil }
    return editorFiles.values.first { $0.file.paneId == paneId }
  }

  func applyEditorSnapshot(_ sections: [String: [JsonValue]], panes: [Pane]) throws {
    let livePanes = Set(
      panes.lazy.filter { $0.mode == .editor || $0.mode == .writing }.map(\.paneId))
    var replacement: [String: EditorFileView] = [:]
    for value in sections["files"] ?? [] {
      let file = try value.decoded(FileDocument.self)
      guard livePanes.contains(file.paneId) else { throw ClientFailure.invalidSnapshot }
      replacement[file.fileId] = EditorFileView(file: file)
    }
    editorFiles = replacement
    if let target = editorNavigationTarget, replacement[target.fileId] == nil {
      editorNavigationTarget = nil
    }
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
      updateEditor(event.fileId) {
        guard $0.file.revision == event.revision else { return }
        $0.diagnostics = event.items
        $0.diagnosticRevision = event.revision
      }
    case .editorAgentCursor:
      let event = try params.decoded(EditorAgentCursorEvent.self)
      updateEditor(event.fileId) { $0.agentCursors[event.paneId] = event }
    case .editorReviewRequested:
      let event = try params.decoded(EditorReviewRequestedEvent.self)
      updateEditor(event.fileId) {
        guard $0.file.revision == event.revision else { return }
        $0.review = event.hunks.isEmpty ? nil : event
      }
    case .editorTabsUpdated, .projectEntryRenamed, .projectTreeChanged:
      try applyEditorWorkspaceEvent(eventName, params)
    default: break
    }
  }

  func discardEditorPane(_ paneId: String) {
    let discardedFileIds = Set(
      editorFiles.values.lazy.filter { $0.file.paneId == paneId }.map(\.file.fileId))
    for fileId in discardedFileIds {
      editorRefreshTasks[fileId]?.cancel()
      editorRefreshTasks[fileId] = nil
      editorRefreshGeneration[fileId] = nil
    }
    editorFiles = editorFiles.filter { $0.value.file.paneId != paneId }
    if let target = editorNavigationTarget, discardedFileIds.contains(target.fileId) {
      editorNavigationTarget = nil
    }
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
    workspaceId: String, path: String, line: UInt32? = nil, column: UInt32? = nil,
    preview: Bool = false
  ) async throws -> FileOpenResult {
    let result = try await mutation(
      .fileOpen,
      FileOpenParams(
        workspaceId: workspaceId, path: path, line: line, column: column,
        preview: preview ? true : nil, idempotencyKey: UUID().uuidString)
    ).decoded(FileOpenResult.self)
    upsertPane(result.pane)
    acceptEditorRead(result.viewport)
    if line != nil || column != nil {
      editorNavigationTarget = EditorNavigationTarget(
        id: UUID().uuidString, fileId: result.file.fileId,
        position: TextPosition(line: result.viewport.fromLine, column: column ?? 0))
    } else {
      editorNavigationTarget = nil
    }
    activateEditorTab(result.pane.paneId)
    return result
  }

  @discardableResult
  public func readEditorFile(
    fileId: String, revision: String? = nil, source: EditorContentSource? = nil,
    versionId: String? = nil, fromLine: UInt32, lineCount: UInt32
  ) async throws -> FileReadResult {
    let result = try await request(
      .fileRead,
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

  public func loadEditorViewport(fileId: String, fromLine: UInt32) async throws {
    if let task = editorWriteTasks[fileId] { await task.value }
    guard let revision = editorFiles[fileId]?.file.revision else {
      throw ClientFailure.invalidSnapshot
    }
    _ = try await readEditorFile(
      fileId: fileId, revision: revision, fromLine: fromLine,
      lineCount: EditorViewportPaging.pageLines)
  }

  @discardableResult
  public func writeEditorFile(
    fileId: String, baseRevision: String, edits: [TextEdit]
  ) async throws -> FileWriteResult {
    guard let actor = connectionActor else { throw ClientFailure.disconnected }
    let result = try await mutation(
      .fileWrite,
      FileWriteParams(
        fileId: fileId, baseRevision: baseRevision, edits: edits,
        author: FileAuthor(actor: actor, name: "you"), idempotencyKey: UUID().uuidString)
    ).decoded(FileWriteResult.self)
    upsertEditor(result.file) { $0.changedLines = result.changedLines }
    return result
  }

  public func enqueueEditorEdits(fileId: String, edits: [TextEdit]) {
    guard !edits.isEmpty else { return }
    invalidateEditorRefresh(fileId)
    editorWriteQueues[fileId, default: []].append(edits)
    startEditorWriteTask(fileId)
  }

  func settleEditorWrites(_ fileId: String) async {
    if let task = editorWriteTasks[fileId] { await task.value }
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
      .fileWatch,
      FileWatchParams(fileId: fileId, watch: watch, idempotencyKey: UUID().uuidString)
    ).decoded(FileWatchResult.self)
    updateEditor(fileId) { $0.file.watching = result.watching }
    return result
  }

  public func editorHistory(
    _ fileId: String, cursor: String? = nil, limit: UInt32 = 50
  ) async throws -> FileHistoryResult {
    try await request(
      .fileHistory,
      .encoded(FileHistoryParams(fileId: fileId, cursor: cursor, limit: limit))
    )
    .decoded(FileHistoryResult.self)
  }

  public func previewEditorFile(
    fileId: String, revision: String? = nil, source: EditorContentSource,
    versionId: String? = nil,
    fromLine: UInt32 = 0, lineCount: UInt32 = UInt32(ProtocolLimits.editorViewportLines)
  ) async throws -> FileReadResult {
    try await request(
      .fileRead,
      .encoded(
        FileReadParams(
          fileId: fileId, revision: revision, source: source, versionId: versionId,
          fromLine: fromLine,
          lineCount: min(lineCount, UInt32(ProtocolLimits.editorViewportLines))))
    )
    .decoded(FileReadResult.self)
  }

  public func saveEditorFile(_ fileId: String) async throws {
    if let task = editorWriteTasks[fileId] { await task.value }
    for _ in 0..<30 {
      try await refreshEditorFile(fileId)
      guard editorFiles[fileId]?.file.dirty == true else { return }
      try await Task.sleep(for: .milliseconds(100))
    }
    throw ClientFailure.editorCommand("The file did not finish saving before the deadline.")
  }

  public func reloadEditorFile(_ fileId: String) async throws {
    if let task = editorWriteTasks[fileId] { await task.value }
    try await refreshEditorFile(fileId)
  }

  @discardableResult
  public func substituteEditorFile(
    fileId: String, baseRevision: String, pattern: String, replacement: String,
    everyMatch: Bool
  ) async throws -> FileWriteResult? {
    guard !pattern.isEmpty else {
      throw ClientFailure.editorCommand("The substitution pattern cannot be empty.")
    }
    var fromLine: UInt32 = 0
    var edits: [TextEdit] = []
    while true {
      let page = try await previewEditorFile(
        fileId: fileId, revision: baseRevision, source: .buffer, fromLine: fromLine)
      guard page.file.revision == baseRevision else {
        throw ClientFailure.editorCommand("The file changed before substitution completed.")
      }
      guard
        let pageEdits = EditorVimSubstitution.edits(
          lines: page.lines, pattern: pattern, replacement: replacement,
          everyMatch: everyMatch, remainingLimit: 4096 - edits.count)
      else {
        throw ClientFailure.editorCommand("The substitution exceeds the bounded edit budget.")
      }
      edits.append(contentsOf: pageEdits)
      if page.toLine >= page.file.lineCount { break }
      guard page.toLine > fromLine else {
        throw ClientFailure.editorCommand("The engine returned an incomplete editor page.")
      }
      fromLine = page.toLine
    }
    guard !edits.isEmpty else { return nil }
    guard try JSONEncoder().encode(edits).count <= ProtocolLimits.editorEditBytes else {
      throw ClientFailure.editorCommand("The substitution exceeds the bounded edit budget.")
    }
    let result = try await writeEditorFile(
      fileId: fileId, baseRevision: baseRevision, edits: edits)
    try await refreshEditorFile(fileId)
    return result
  }

  /// Undoes or redoes the newest edit transaction the engine recorded for this
  /// file, then reloads the page holding the restored text and places the cursor
  /// there. The history is engine-owned, so every paired client shares it.
  @discardableResult
  public func replayEditorHistory(
    _ fileId: String, direction: EditorHistoryDirection
  ) async throws -> FileUndoResult {
    await settleEditorWrites(fileId)
    invalidateEditorRefresh(fileId)
    guard let revision = editorFiles[fileId]?.file.revision else {
      throw ClientFailure.invalidSnapshot
    }
    let result = try await mutation(
      direction.method,
      FileUndoParams(fileId: fileId, baseRevision: revision, idempotencyKey: UUID().uuidString)
    ).decoded(FileUndoResult.self)
    upsertEditor(result.file) { $0.changedLines = result.changedLines }
    guard result.applied else { return result }
    try await reloadEditorPage(fileId, showing: result.cursor?.line)
    if let cursor = result.cursor {
      editorNavigationTarget = EditorNavigationTarget(
        id: UUID().uuidString, fileId: fileId, position: cursor)
    }
    return result
  }

  /// Reloads the page after an engine-side change, moving it only when
  /// `line` lies outside the loaded page.
  func reloadEditorPage(_ fileId: String, showing line: UInt32?) async throws {
    guard let loaded = editorFiles[fileId]?.viewport else { return }
    let fromLine = EditorViewportPaging.landingStart(
      line: line ?? loaded.fromLine, loadedFrom: loaded.fromLine, loadedTo: loaded.toLine)
    _ = try await readEditorFile(
      fileId: fileId, fromLine: fromLine, lineCount: EditorViewportPaging.pageLines)
  }

  @discardableResult
  public func keepEditorDraft(fileId: String, baseRevision: String) async throws
    -> FileWriteResult
  {
    let origin = TextPosition(line: 0, column: 0)
    return try await writeEditorFile(
      fileId: fileId, baseRevision: baseRevision,
      edits: [TextEdit(range: TextRange(start: origin, end: origin), text: "")])
  }

  @discardableResult
  public func revertEditorFile(
    _ fileId: String, versionId: String, baseRevision: String
  ) async throws -> FileRevertResult {
    let result = try await mutation(
      .fileRevert,
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
      .fileSearch,
      FileSearchParams(
        workspaceId: workspaceId, scope: scope, query: query, replacement: replacement,
        caseSensitive: caseSensitive,
        limit: min(limit, UInt32(ProtocolLimits.editorSearchResults)),
        idempotencyKey: UUID().uuidString)
    )
    .decoded(FileSearchResult.self)
  }

  public func editorSymbols(fileId: String, revision: String) async throws -> FileSymbolsResult {
    try await request(
      .fileSymbols, .encoded(FileSymbolsParams(fileId: fileId, revision: revision))
    ).decoded(FileSymbolsResult.self)
  }

  public func editorDefinition(
    fileId: String, revision: String, position: TextPosition
  ) async throws -> FileDefinitionResult {
    try await request(
      .fileDefinition,
      .encoded(FilePositionParams(fileId: fileId, revision: revision, position: position))
    ).decoded(FileDefinitionResult.self)
  }

  public func editorReferences(
    fileId: String, revision: String, position: TextPosition
  ) async throws -> FileReferencesResult {
    try await request(
      .fileReferences,
      .encoded(FilePositionParams(fileId: fileId, revision: revision, position: position))
    ).decoded(FileReferencesResult.self)
  }

  public func editorDiagnostics(
    fileId: String, revision: String
  ) async throws -> FileDiagnosticsResult {
    try await request(
      .fileDiagnostics, .encoded(FileDiagnosticsParams(fileId: fileId, revision: revision))
    ).decoded(FileDiagnosticsResult.self)
  }

  public func editorDiff(fileId: String, revision: String) async throws -> FileDiffResult {
    try await request(
      .fileDiff, .encoded(FileDiffParams(fileId: fileId, revision: revision))
    ).decoded(FileDiffResult.self)
  }

  @discardableResult
  public func acceptEditorReview(
    fileId: String, hunkId: String, baseRevision: String
  ) async throws -> EditorReviewResult {
    try await resolveEditorReview(
      method: .editorReviewAccept, fileId: fileId, hunkId: hunkId,
      baseRevision: baseRevision)
  }

  @discardableResult
  public func rejectEditorReview(
    fileId: String, hunkId: String, baseRevision: String
  ) async throws -> EditorReviewResult {
    try await resolveEditorReview(
      method: .editorReviewReject, fileId: fileId, hunkId: hunkId,
      baseRevision: baseRevision)
  }

  public func commentOnEditorReview(
    fileId: String, range: SoyehtProtocol.TextRange, body: String
  ) async throws -> EditorReviewCommentResult {
    try await mutation(
      .editorReviewComment,
      EditorReviewCommentParams(
        fileId: fileId, range: range, body: body, idempotencyKey: UUID().uuidString)
    ).decoded(EditorReviewCommentResult.self)
  }

  public func languageServerStatus(
    workspaceId: String? = nil, language: EditorLanguage? = nil
  ) async throws -> LspStatusResult {
    try await request(
      .lspStatus, .encoded(LspStatusParams(workspaceId: workspaceId, language: language))
    ).decoded(LspStatusResult.self)
  }

  @discardableResult
  public func formatEditorFile(fileId: String, baseRevision: String) async throws
    -> FileFormatResult
  {
    let result = try await mutation(
      .fileFormat,
      FileFormatParams(
        fileId: fileId, baseRevision: baseRevision, idempotencyKey: UUID().uuidString)
    ).decoded(FileFormatResult.self)
    upsertEditor(result.file)
    try await refreshEditorFile(fileId)
    return result
  }

  @discardableResult
  public func openEditorLocation(_ location: FileLocation) async throws -> FileOpenResult {
    try await openEditorFile(
      workspaceId: location.workspaceId, path: location.path, line: location.range.start.line,
      column: location.range.start.column)
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
        // Best effort: the problem above is what the person sees; a reload retries later.
        try? await refreshEditorFile(fileId)
        return
      }
    }
    editorWriteQueues[fileId] = nil
    scheduleEditorReload(fileId)
  }

  private func scheduleEditorReload(_ fileId: String) {
    guard connected, editorFiles[fileId]?.viewport != nil else { return }
    let generation = nextEditorRefreshGeneration(fileId)
    editorRefreshTasks[fileId]?.cancel()
    editorRefreshTasks[fileId] = Task { [weak self] in
      do { try await Task.sleep(for: .milliseconds(180)) } catch { return }
      guard let self else { return }
      await self.settleEditorWrites(fileId)
      guard self.editorRefreshGeneration[fileId] == generation,
        self.editorWriteQueues[fileId]?.isEmpty != false,
        self.editorWriteTasks[fileId] == nil
      else { return }
      do {
        try await self.refreshEditorFile(fileId, generation: generation)
      } catch is CancellationError {} catch {
        guard self.editorRefreshGeneration[fileId] == generation else { return }
        self.problem = error.localizedDescription
      }
      if self.editorRefreshGeneration[fileId] == generation {
        self.editorRefreshTasks[fileId] = nil
      }
    }
  }

  func invalidateEditorRefresh(_ fileId: String) {
    _ = nextEditorRefreshGeneration(fileId)
    editorRefreshTasks[fileId]?.cancel()
    editorRefreshTasks[fileId] = nil
  }

  private func nextEditorRefreshGeneration(_ fileId: String) -> UInt64 {
    let next = (editorRefreshGeneration[fileId] ?? 0) &+ 1
    editorRefreshGeneration[fileId] = next
    return next
  }

  private func refreshEditorFile(_ fileId: String, generation: UInt64) async throws {
    guard let viewport = editorFiles[fileId]?.viewport else { return }
    let result = try await previewEditorFile(
      fileId: fileId, source: .buffer, fromLine: viewport.fromLine,
      lineCount: max(1, viewport.toLine - viewport.fromLine))
    guard editorRefreshGeneration[fileId] == generation,
      editorWriteQueues[fileId]?.isEmpty != false,
      editorWriteTasks[fileId] == nil
    else { return }
    acceptEditorRead(result)
  }

  func acceptEditorRead(_ result: FileReadResult) {
    upsertEditor(result.file) { $0.viewport = result }
  }

  func upsertEditor(
    _ file: FileDocument, update: (inout EditorFileView) -> Void = { _ in }
  ) {
    var view = editorFiles[file.fileId] ?? EditorFileView(file: file)
    if view.diagnosticRevision != file.revision {
      view.diagnostics = []
      view.diagnosticRevision = nil
    }
    if view.review?.revision != file.revision { view.review = nil }
    view.file = file
    update(&view)
    editorFiles[file.fileId] = view
    synchronizeWritingFile(file)
  }

  private func updateEditor(_ fileId: String, update: (inout EditorFileView) -> Void) {
    guard var view = editorFiles[fileId] else { return }
    update(&view)
    editorFiles[fileId] = view
  }

  private func resolveEditorReview(
    method: RequestMethod, fileId: String, hunkId: String, baseRevision: String
  ) async throws -> EditorReviewResult {
    let result = try await mutation(
      method,
      EditorReviewParams(
        fileId: fileId, hunkId: hunkId, baseRevision: baseRevision,
        idempotencyKey: UUID().uuidString)
    ).decoded(EditorReviewResult.self)
    upsertEditor(result.file)
    try await refreshEditorFile(fileId)
    return result
  }
}
