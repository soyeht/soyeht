import Foundation
import SoyehtProtocol

public struct WritingDocumentView: Sendable {
  public var document: WritingDocument
  public var structure: WritingStructureResult?

  public init(document: WritingDocument, structure: WritingStructureResult? = nil) {
    self.document = document
    self.structure = structure
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
    view.structure = nil
    writingDocuments[event.fileId] = view
    if var file = editorFiles[event.fileId] {
      file.file.revision = event.revision
      editorFiles[event.fileId] = file
    }
    guard connected else { return }
    perform { try await self.reloadWritingDocument(event.fileId) }
  }

  func discardWritingPane(_ paneId: String) {
    writingDocuments = writingDocuments.filter { $0.value.document.file.paneId != paneId }
  }

  public func loadWritingPane(_ paneId: String) async throws {
    guard let view = writingDocuments.values.first(where: { $0.document.file.paneId == paneId })
    else { throw ClientFailure.invalidSnapshot }
    _ = try await readEditorFile(
      fileId: view.document.file.fileId, revision: view.document.file.revision,
      fromLine: 0, lineCount: UInt32(ProtocolLimits.editorViewportLines))
    _ = try await loadWritingStructure(fileId: view.document.file.fileId)
  }

  public func listWritingSection(
    workspaceId: String, section: WritingSection, cursor: String? = nil, limit: UInt32 = 100
  ) async throws -> WritingSectionListResult {
    try await request(
      "writing.section.list",
      .encoded(
        WritingSectionListParams(
          workspaceId: workspaceId, section: section, cursor: cursor,
          limit: min(limit, UInt32(ProtocolLimits.writingSectionEntries))))
    )
    .decoded(WritingSectionListResult.self)
  }

  @discardableResult
  public func createWritingDocument(
    workspaceId: String, section: WritingSection, title: String, relativeFolder: String? = nil
  ) async throws -> WritingDocumentCreateResult {
    let result = try await mutation(
      "writing.document.create",
      WritingDocumentCreateParams(
        workspaceId: workspaceId, section: section, title: title,
        relativeFolder: relativeFolder, idempotencyKey: UUID().uuidString)
    )
    .decoded(WritingDocumentCreateResult.self)
    acceptWritingOpen(document: result.document, pane: result.pane, viewport: result.viewport)
    return result
  }

  @discardableResult
  public func openWritingDocument(
    workspaceId: String, path: String, kind: WritingKind
  ) async throws -> WritingDocumentOpenResult {
    let result = try await mutation(
      "writing.document.open",
      WritingDocumentOpenParams(
        workspaceId: workspaceId, path: path, kind: kind,
        idempotencyKey: UUID().uuidString)
    )
    .decoded(WritingDocumentOpenResult.self)
    acceptWritingOpen(document: result.document, pane: result.pane, viewport: result.viewport)
    return result
  }

  @discardableResult
  public func loadWritingStructure(fileId: String) async throws -> WritingStructureResult {
    guard let revision = writingDocuments[fileId]?.document.file.revision else {
      throw ClientFailure.invalidSnapshot
    }
    let result = try await request(
      "writing.structure", .encoded(WritingStructureParams(fileId: fileId, revision: revision))
    )
    .decoded(WritingStructureResult.self)
    guard result.document.file.revision == revision else { throw ClientFailure.invalidFrame }
    writingDocuments[fileId] = WritingDocumentView(document: result.document, structure: result)
    return result
  }

  @discardableResult
  public func formatWritingDocument(
    fileId: String, range: SoyehtProtocol.TextRange, kind: WritingFormatKind,
    value: String? = nil
  ) async throws -> WritingFormatResult {
    guard let revision = writingDocuments[fileId]?.document.file.revision else {
      throw ClientFailure.invalidSnapshot
    }
    let result = try await mutation(
      "writing.format",
      WritingFormatParams(
        fileId: fileId, baseRevision: revision, range: range, kind: kind, value: value,
        idempotencyKey: UUID().uuidString)
    )
    .decoded(WritingFormatResult.self)
    acceptWritingFile(result.file)
    _ = try await readEditorFile(
      fileId: result.file.fileId, revision: result.file.revision, fromLine: 0,
      lineCount: UInt32(ProtocolLimits.editorViewportLines))
    return result
  }

  public func readWritingAsset(fileId: String, asset: String) async throws -> Data {
    guard let revision = writingDocuments[fileId]?.document.file.revision else {
      throw ClientFailure.invalidSnapshot
    }
    let response = try await requestResponse(
      "writing.asset.read",
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

  @discardableResult
  public func setWritingPreferences(
    fileId: String, font: WritingFont? = nil, focusMode: Bool? = nil
  ) async throws -> WritingPreferencesResult {
    guard let revision = writingDocuments[fileId]?.document.file.revision else {
      throw ClientFailure.invalidSnapshot
    }
    let result = try await mutation(
      "writing.preferences.set",
      WritingPreferencesParams(
        fileId: fileId, baseRevision: revision, font: font, focusMode: focusMode,
        idempotencyKey: UUID().uuidString)
    )
    .decoded(WritingPreferencesResult.self)
    writingDocuments[fileId] = WritingDocumentView(document: result.document)
    return result
  }

  private func acceptWritingOpen(
    document: WritingDocument, pane: Pane, viewport: FileReadResult
  ) {
    upsertPane(pane)
    acceptEditorRead(viewport)
    writingDocuments[document.file.fileId] = WritingDocumentView(document: document)
    select(pane.paneId)
  }

  private func acceptWritingFile(_ file: FileDocument) {
    upsertEditor(file)
    guard var view = writingDocuments[file.fileId] else { return }
    view.document.file = file
    view.structure = nil
    writingDocuments[file.fileId] = view
  }

  private func reloadWritingDocument(_ fileId: String) async throws {
    guard writingDocuments[fileId] != nil else { return }
    _ = try await loadWritingStructure(fileId: fileId)
    guard let revision = writingDocuments[fileId]?.document.file.revision else { return }
    _ = try await readEditorFile(
      fileId: fileId, revision: revision, fromLine: 0,
      lineCount: UInt32(ProtocolLimits.editorViewportLines))
  }
}
