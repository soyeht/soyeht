import Foundation
import SoyehtProtocol

extension EngineClient {
  func applyPaneCreated(_ event: PaneCreatedEvent) throws {
    let pane = event.pane
    switch pane.mode {
    case .editor:
      if event.file == nil, event.writingDocument == nil { break }
      guard let file = event.file, event.writingDocument == nil,
        file.paneId == pane.paneId, file.workspaceId == pane.workspaceId
      else { throw ClientFailure.invalidFrame }
      upsertEditor(file)
    case .writing:
      if event.file == nil, event.writingDocument == nil { break }
      guard let file = event.file, let document = event.writingDocument,
        file.paneId == pane.paneId, file.workspaceId == pane.workspaceId,
        try JsonValue.encoded(file) == JsonValue.encoded(document.file),
        pane.writingKind == document.kind, pane.writingSection == document.section
      else { throw ClientFailure.invalidFrame }
      upsertEditor(file)
      writingDocuments[file.fileId] = WritingDocumentView(document: document)
    case .terminal, .agent, .web:
      guard event.file == nil, event.writingDocument == nil else {
        throw ClientFailure.invalidFrame
      }
    }
    acceptPane(pane)
  }
}
