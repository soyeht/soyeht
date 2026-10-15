import Foundation
import SoyehtProtocol

public struct WritingDocumentView: Sendable {
  public var document: WritingDocument
  public var structure: WritingStructureResult?

  public init(document: WritingDocument, structure: WritingStructureResult? = nil) {
    self.document = document
    self.structure = structure
  }

  public var loadIdentity: String {
    "\(document.file.paneId):\(document.file.fileId)"
  }
}

extension EngineClient {
  public var selectedWritingDocument: WritingDocumentView? {
    guard let paneId = selectedPaneId else { return nil }
    return writingDocuments.values.first { $0.document.file.paneId == paneId }
  }

  public var selectedWritingFile: EditorFileView? {
    guard let fileId = selectedWritingDocument?.document.file.fileId else { return nil }
    return editorFiles[fileId]
  }

  func applyWritingSnapshot(_ sections: [String: [JsonValue]], panes: [Pane]) throws {
    let live = Dictionary(
      uniqueKeysWithValues: panes.lazy.filter { $0.mode == .writing }.map { ($0.paneId, $0) })
    var replacement: [String: WritingDocumentView] = [:]
    for value in sections["writing"] ?? [] {
      let document = try value.decoded(WritingDocument.self)
      guard let pane = live[document.file.paneId],
        pane.writingKind == document.kind,
        pane.writingSection == document.section,
        editorFiles[document.file.fileId]?.file.paneId == pane.paneId
      else { throw ClientFailure.invalidSnapshot }
      replacement[document.file.fileId] = WritingDocumentView(document: document)
    }
    writingDocuments = replacement
  }

  func applyWritingEvent(_ params: JsonValue) throws {
    let event = try params.decoded(WritingChangedEvent.self)
    guard var view = writingDocuments[event.fileId] else { return }
    guard view.document.file.paneId == event.paneId else {
      throw ClientFailure.invalidFrame
    }
    view.document.file.revision = event.revision
    view.document.sidecarRevision = event.sidecarRevision
    writingDocuments[event.fileId] = view
    if var file = editorFiles[event.fileId] {
      file.file.revision = event.revision
      editorFiles[event.fileId] = file
    }
    scheduleWritingReload(event.fileId)
  }

  func discardWritingPane(_ paneId: String) {
    let fileIds = writingDocuments.values.lazy.filter {
      $0.document.file.paneId == paneId
    }.map(\.document.file.fileId)
    for fileId in fileIds {
      writingRefreshTasks[fileId]?.cancel()
      writingRefreshTasks[fileId] = nil
      writingRefreshGeneration[fileId] = nil
    }
    writingDocuments = writingDocuments.filter { $0.value.document.file.paneId != paneId }
  }

  public func loadWritingPane(_ paneId: String) async throws {
    guard let view = writingDocuments.values.first(where: { $0.document.file.paneId == paneId })
    else { throw ClientFailure.invalidSnapshot }
    await settleEditorWrites(view.document.file.fileId)
    _ = try await readEditorFile(
      fileId: view.document.file.fileId,
      fromLine: 0, lineCount: UInt32(ProtocolLimits.editorViewportLines))
    _ = try await loadWritingStructure(fileId: view.document.file.fileId)
  }

  public func listWritingSection(
    workspaceId: String, section: WritingSection, cursor: String? = nil, limit: UInt32 = 100
  ) async throws -> WritingSectionListResult {
    try await request(
      .writingSectionList,
      .encoded(
        WritingSectionListParams(
          workspaceId: workspaceId, section: section, cursor: cursor,
          limit: min(limit, UInt32(ProtocolLimits.writingSectionEntries))))
    )
    .decoded(WritingSectionListResult.self)
  }

  @discardableResult
  public func createWritingDocument(
    workspaceId: String, section: WritingSection, title: String, relativeFolder: String? = nil,
    selectPane: Bool = true
  ) async throws -> WritingDocumentCreateResult {
    let result = try await mutation(
      .writingDocumentCreate,
      WritingDocumentCreateParams(
        workspaceId: workspaceId, section: section, title: title,
        relativeFolder: relativeFolder, idempotencyKey: UUID().uuidString)
    )
    .decoded(WritingDocumentCreateResult.self)
    acceptWritingOpen(
      document: result.document, pane: result.pane, viewport: result.viewport,
      selectPane: selectPane)
    return result
  }

  @discardableResult
  public func openWritingDocument(
    workspaceId: String, path: String, kind: WritingKind, selectPane: Bool = true
  ) async throws -> WritingDocumentOpenResult {
    let result = try await mutation(
      .writingDocumentOpen,
      WritingDocumentOpenParams(
        workspaceId: workspaceId, path: path, kind: kind,
        idempotencyKey: UUID().uuidString)
    )
    .decoded(WritingDocumentOpenResult.self)
    acceptWritingOpen(
      document: result.document, pane: result.pane, viewport: result.viewport,
      selectPane: selectPane)
    return result
  }

  @discardableResult
  public func loadWritingStructure(fileId: String) async throws -> WritingStructureResult {
    let result: WritingStructureResult = try await withWritingRevision(fileId) { revision in
      let value = try await self.request(
        .writingStructure, .encoded(WritingStructureParams(fileId: fileId, revision: revision))
      ).decoded(WritingStructureResult.self)
      guard value.document.file.revision == revision else { throw ClientFailure.invalidFrame }
      return value
    }
    writingDocuments[fileId] = WritingDocumentView(document: result.document, structure: result)
    return result
  }

  @discardableResult
  public func formatWritingDocument(
    fileId: String, range: SoyehtProtocol.TextRange, kind: WritingFormatKind,
    value: String? = nil, image: WebHostAttachment? = nil
  ) async throws -> WritingFormatResult {
    let key = UUID().uuidString
    let result: WritingFormatResult = try await withWritingRevision(fileId) { revision in
      try await self.mutation(
        .writingFormat,
        WritingFormatParams(
          fileId: fileId, baseRevision: revision, range: range, kind: kind, value: value,
          image: image?.reference, idempotencyKey: key),
        attachments: image.map { [$0] } ?? []
      ).decoded(WritingFormatResult.self)
    }
    acceptWritingFile(result.file)
    _ = try await readEditorFile(
      fileId: result.file.fileId, revision: result.file.revision, fromLine: 0,
      lineCount: UInt32(ProtocolLimits.editorViewportLines))
    return result
  }

  public func readWritingAsset(fileId: String, asset: String) async throws -> Data {
    try await withWritingRevision(fileId) { revision in
      let response = try await self.requestResponse(
        .writingAssetRead,
        .encoded(WritingAssetReadParams(fileId: fileId, revision: revision, asset: asset)))
      let result = try response.value.decoded(WritingAssetReadResult.self)
      guard result.fileId == fileId, result.revision == revision,
        result.image.mimeType == "image/png",
        result.image.name == String(asset.dropFirst("assets/".count)),
        let attachment = response.attachment,
        attachment.reference.attachmentId == result.image.attachmentId,
        attachment.bytes.starts(with: Data([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])),
        attachment.bytes.count <= ProtocolLimits.writingImageBytes
      else { throw ClientFailure.invalidFrame }
      return attachment.bytes
    }
  }

  @discardableResult
  public func setWritingPreferences(
    fileId: String, font: WritingFont? = nil, focusMode: Bool? = nil,
    fullWidth: Bool? = nil
  ) async throws -> WritingPreferencesResult {
    let key = UUID().uuidString
    let result: WritingPreferencesResult = try await withWritingRevision(fileId) { revision in
      try await self.mutation(
        .writingPreferencesSet,
        WritingPreferencesParams(
          fileId: fileId, baseRevision: revision, font: font, focusMode: focusMode,
          fullWidth: fullWidth, idempotencyKey: key)
      ).decoded(WritingPreferencesResult.self)
    }
    writingDocuments[fileId] = WritingDocumentView(document: result.document)
    return result
  }

  public func requestWritingVariant(
    fileId: String, lineId: String, agentPaneId: String, instruction: String? = nil
  ) async throws -> WritingVariantRequestResult {
    let key = UUID().uuidString
    return try await withWritingRevision(fileId) { revision in
      try await self.mutation(
        .writingVariantRequest,
        WritingVariantRequestParams(
          fileId: fileId, lineId: lineId, baseRevision: revision,
          agentPaneId: agentPaneId, instruction: instruction, idempotencyKey: key)
      ).decoded(WritingVariantRequestResult.self)
    }
  }

  @discardableResult
  public func selectWritingVariant(
    fileId: String, lineId: String, variantId: String
  ) async throws -> WritingVariantResult {
    let key = UUID().uuidString
    let result: WritingVariantResult = try await withWritingRevision(fileId) { revision in
      try await self.mutation(
        .writingVariantSelect,
        WritingVariantSelectParams(
          fileId: fileId, lineId: lineId, variantId: variantId,
          baseRevision: revision, idempotencyKey: key)
      ).decoded(WritingVariantResult.self)
    }
    return try await acceptWritingVariantResult(result)
  }

  @discardableResult
  public func removeWritingVariant(
    fileId: String, lineId: String, variantId: String
  ) async throws -> WritingVariantResult {
    let key = UUID().uuidString
    let result: WritingVariantResult = try await withWritingRevision(fileId) { revision in
      try await self.mutation(
        .writingVariantRemove,
        WritingVariantRemoveParams(
          fileId: fileId, lineId: lineId, variantId: variantId,
          baseRevision: revision, idempotencyKey: key)
      ).decoded(WritingVariantResult.self)
    }
    return try await acceptWritingVariantResult(result)
  }

  public func listWritingBeats(fileId: String) async throws -> WritingBeatListResult {
    let result: WritingBeatListResult = try await withWritingRevision(fileId) { revision in
      try await self.request(
        .writingBeatList, .encoded(WritingBeatListParams(fileId: fileId, revision: revision))
      ).decoded(WritingBeatListResult.self)
    }
    guard result.beats.count <= ProtocolLimits.writingBeatCards else {
      throw ClientFailure.invalidFrame
    }
    return result
  }

  @discardableResult
  public func putWritingBeat(
    fileId: String, beatId: String? = nil, title: String, body: String,
    category: BeatCategory, order: UInt32, sceneId: String? = nil
  ) async throws -> WritingBeatResult {
    let key = UUID().uuidString
    let result: WritingBeatResult = try await withWritingRevision(fileId) { revision in
      try await self.mutation(
        .writingBeatPut,
        WritingBeatPutParams(
          fileId: fileId, baseRevision: revision, beatId: beatId,
          title: title, body: body, category: category, order: order, sceneId: sceneId,
          idempotencyKey: key)
      ).decoded(WritingBeatResult.self)
    }
    try await acceptWritingBeatResult(result, fileId: fileId)
    return result
  }

  @discardableResult
  public func deleteWritingBeat(fileId: String, beatId: String) async throws -> WritingBeatResult {
    let key = UUID().uuidString
    let result: WritingBeatResult = try await withWritingRevision(fileId) { revision in
      try await self.mutation(
        .writingBeatDelete,
        WritingBeatDeleteParams(
          fileId: fileId, baseRevision: revision, beatId: beatId, idempotencyKey: key)
      ).decoded(WritingBeatResult.self)
    }
    try await acceptWritingBeatResult(result, fileId: fileId)
    return result
  }

  private func writingRevision(_ fileId: String) throws -> String {
    guard let revision = writingDocuments[fileId]?.document.file.revision else {
      throw ClientFailure.invalidSnapshot
    }
    return revision
  }

  private func acceptWritingVariantResult(
    _ result: WritingVariantResult
  ) async throws -> WritingVariantResult {
    let fileId = result.document.file.fileId
    writingDocuments[fileId] = WritingDocumentView(document: result.document)
    upsertEditor(result.document.file)
    _ = try await readEditorFile(
      fileId: fileId, revision: result.document.file.revision, fromLine: 0,
      lineCount: UInt32(ProtocolLimits.editorViewportLines))
    _ = try await loadWritingStructure(fileId: fileId)
    return result
  }

  private func withWritingRevision<Value>(
    _ fileId: String, operation: (String) async throws -> Value
  ) async throws -> Value {
    await settleEditorWrites(fileId)
    do { return try await operation(try writingRevision(fileId)) } catch {
      guard case ClientFailure.remote(ErrorKind.stale_version.rawValue, _, _) = error else {
        throw error
      }
      return try await operation(try await refreshWritingRevision(fileId))
    }
  }

  private func refreshWritingRevision(_ fileId: String) async throws -> String {
    let fromLine = editorFiles[fileId]?.viewport?.fromLine ?? 0
    let result = try await readEditorFile(
      fileId: fileId, fromLine: fromLine,
      lineCount: UInt32(ProtocolLimits.editorViewportLines))
    return result.file.revision
  }

  private func acceptWritingBeatResult(_ result: WritingBeatResult, fileId: String) async throws {
    guard result.document.file.fileId == fileId else { throw ClientFailure.invalidFrame }
    writingDocuments[fileId] = WritingDocumentView(document: result.document)
    upsertEditor(result.document.file)
    _ = try await loadWritingStructure(fileId: fileId)
  }

  private func acceptWritingOpen(
    document: WritingDocument, pane: Pane, viewport: FileReadResult, selectPane: Bool
  ) {
    upsertPane(pane)
    acceptEditorRead(viewport)
    writingDocuments[document.file.fileId] = WritingDocumentView(document: document)
    if selectPane { select(pane.paneId) }
  }

  private func acceptWritingFile(_ file: FileDocument) {
    upsertEditor(file)
  }

  func synchronizeWritingFile(_ file: FileDocument) {
    guard var view = writingDocuments[file.fileId] else { return }
    let changed = view.document.file.revision != file.revision
    view.document.file = file
    writingDocuments[file.fileId] = view
    if changed { scheduleWritingReload(file.fileId) }
  }

  private func scheduleWritingReload(_ fileId: String) {
    guard connected, writingDocuments[fileId]?.structure != nil else { return }
    let generation = (writingRefreshGeneration[fileId] ?? 0) &+ 1
    writingRefreshGeneration[fileId] = generation
    writingRefreshTasks[fileId]?.cancel()
    writingRefreshTasks[fileId] = Task { [weak self] in
      do { try await Task.sleep(for: .milliseconds(240)) } catch { return }
      guard let self else { return }
      await self.settleEditorWrites(fileId)
      guard self.writingRefreshGeneration[fileId] == generation else { return }
      do {
        try await self.refreshWritingStructure(fileId, generation: generation)
      } catch is CancellationError {} catch {
        guard self.writingRefreshGeneration[fileId] == generation else { return }
        self.problem = error.localizedDescription
      }
      if self.writingRefreshGeneration[fileId] == generation {
        self.writingRefreshTasks[fileId] = nil
      }
    }
  }

  private func refreshWritingStructure(_ fileId: String, generation: UInt64) async throws {
    let revision = try writingRevision(fileId)
    let result = try await request(
      .writingStructure, .encoded(WritingStructureParams(fileId: fileId, revision: revision))
    ).decoded(WritingStructureResult.self)
    guard result.document.file.revision == revision,
      writingRefreshGeneration[fileId] == generation,
      writingDocuments[fileId]?.document.file.revision == revision
    else { return }
    writingDocuments[fileId] = WritingDocumentView(document: result.document, structure: result)
  }
}
