import Foundation
import SoyehtProtocol

/// A file attached in the composer and not yet sent. The engine decides how each
/// file reaches the agent; the client only describes it and delivers its bytes.
public struct AgentAttachmentDraft: Identifiable, Sendable, Equatable {
  public enum Source: Sendable, Equatable {
    /// Bytes sent through the engine as binary frames, so they work from any device.
    case bytes(Data)
    /// A file already on the engine's Mac, linked where it is and never copied.
    case hostFile(String)
  }

  public let id: UUID
  public let kind: PromptAttachmentKind
  public let name: String
  public let mimeType: String
  public let byteCount: Int
  public let source: Source
  /// A small PNG or JPEG preview kept with the transcript.
  public let thumbnail: Data?

  public init(
    id: UUID = UUID(), kind: PromptAttachmentKind, name: String, mimeType: String,
    byteCount: Int, source: Source, thumbnail: Data? = nil
  ) {
    self.id = id
    self.kind = kind
    self.name = name
    self.mimeType = mimeType
    self.byteCount = byteCount
    self.source = source
    self.thumbnail = thumbnail
  }

  /// Bytes this draft sends through the engine; a linked host file sends none.
  var uploadedBytes: Int {
    if case .bytes(let data) = source { data.count } else { 0 }
  }

  /// The protocol description and, for bytes, the frames that carry them.
  func wire() -> (PromptAttachment, WebHostAttachment?) {
    let attachmentId = id.uuidString.lowercased()
    let thumbnail = thumbnail?.base64EncodedString()
    switch source {
    case .bytes(let data):
      let size = String(data.count)
      return (
        PromptAttachment(
          kind: kind, name: name, mimeType: mimeType, bytes: size, attachmentId: attachmentId,
          thumbnail: thumbnail),
        WebHostAttachment(
          reference: AttachmentRef(
            attachmentId: attachmentId, mimeType: mimeType, name: name, bytes: size),
          bytes: data)
      )
    case .hostFile(let path):
      return (
        PromptAttachment(
          kind: kind, name: name, mimeType: mimeType, bytes: String(byteCount), path: path,
          thumbnail: thumbnail),
        nil
      )
    }
  }
}

/// A staged draft, or why the composer refused that file.
public typealias AgentAttachmentResult = Result<AgentAttachmentDraft, AgentAttachmentRejection>

/// Why the composer refused a file. The engine enforces the same limits.
public enum AgentAttachmentRejection: Error, Equatable, Sendable {
  case tooMany
  case tooLarge(name: String)
  case totalTooLarge
  case empty(name: String)
  case unreadable(name: String)
  case folder(name: String)

  public var message: String {
    switch self {
    case .tooMany:
      "A message takes at most \(ProtocolLimits.promptAttachments) attachments."
    case .tooLarge(let name):
      "\(name) is larger than \(AgentAttachmentList.mebibytes(ProtocolLimits.promptAttachmentBytes)) MB."
    case .totalTooLarge:
      "Attachments are limited to \(AgentAttachmentList.mebibytes(ProtocolLimits.promptAttachmentTotalBytes)) MB per message."
    case .empty(let name): "\(name) is empty."
    case .unreadable(let name): "\(name) could not be read."
    case .folder(let name): "\(name) is a folder; attach the files inside it."
    }
  }
}

/// The composer's attachments for its next prompt, kept within the engine's limits.
public struct AgentAttachmentList: Sendable, Equatable {
  public private(set) var items: [AgentAttachmentDraft] = []

  public init() {}

  public var isEmpty: Bool { items.isEmpty }

  public mutating func add(_ draft: AgentAttachmentDraft) throws(AgentAttachmentRejection) {
    guard items.count < ProtocolLimits.promptAttachments else { throw .tooMany }
    if case .bytes(let data) = draft.source, data.isEmpty { throw .empty(name: draft.name) }
    guard draft.uploadedBytes <= ProtocolLimits.promptAttachmentBytes else {
      throw .tooLarge(name: draft.name)
    }
    let total = items.reduce(draft.uploadedBytes) { $0 + $1.uploadedBytes }
    guard total <= ProtocolLimits.promptAttachmentTotalBytes else { throw .totalTooLarge }
    items.append(draft)
  }

  public mutating func remove(_ id: UUID) {
    items.removeAll { $0.id == id }
  }

  public mutating func removeAll() {
    items.removeAll()
  }

  static func mebibytes(_ bytes: Int) -> Int { bytes / (1_024 * 1_024) }
}

extension EngineClient {
  /// What `pane.prompt` carries: the picked page elements' images first, then the
  /// composer's files, with the frames for every attachment sent as bytes.
  nonisolated static func promptAttachments(
    web: AgentWebAttachmentDraft?, drafts: [AgentAttachmentDraft]
  ) -> ([PromptAttachment], [WebHostAttachment]) {
    var attachments: [PromptAttachment] = []
    var frames: [WebHostAttachment] = []
    for image in web?.images ?? [] {
      let reference = image.reference
      attachments.append(
        PromptAttachment(
          kind: .image, name: reference.name, mimeType: reference.mimeType,
          bytes: reference.bytes, attachmentId: reference.attachmentId))
      frames.append(image)
    }
    for draft in drafts {
      let (attachment, upload) = draft.wire()
      attachments.append(attachment)
      if let upload { frames.append(upload) }
    }
    return (attachments, frames)
  }
}
