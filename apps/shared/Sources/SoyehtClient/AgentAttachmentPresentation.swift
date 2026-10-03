import Foundation
import SoyehtProtocol

/// How one attachment chip reads, in the composer and in a sent message.
struct AgentAttachmentPresentation: Identifiable, Equatable, Sendable {
  let id: String
  let name: String
  let systemImage: String
  let detail: String
  let thumbnail: Data?

  init(draft: AgentAttachmentDraft) {
    id = draft.id.uuidString
    name = draft.name
    systemImage = Self.systemImage(kind: draft.kind, mimeType: draft.mimeType)
    detail = Self.detail(mimeType: draft.mimeType, bytes: draft.byteCount)
    thumbnail = draft.thumbnail
  }

  /// A sent attachment; the transcript keeps its name, size and thumbnail.
  init(attachment: PromptAttachment, index: Int) {
    id = "\(index)-\(attachment.path ?? attachment.name)"
    name = attachment.name
    systemImage = Self.systemImage(kind: attachment.kind, mimeType: attachment.mimeType)
    detail = Self.detail(mimeType: attachment.mimeType, bytes: Int(attachment.bytes) ?? 0)
    thumbnail = attachment.thumbnail.flatMap { Data(base64Encoded: $0) }
  }

  static func systemImage(kind: PromptAttachmentKind, mimeType: String) -> String {
    switch kind {
    case .image: "photo"
    case .audio: "waveform"
    case .text: "doc.text"
    case .file: mimeType == "application/pdf" ? "doc.richtext" : "doc"
    }
  }

  /// "PNG · 12 KB": the subtype of the MIME type and a file-style size.
  static func detail(mimeType: String, bytes: Int) -> String {
    let subtype = mimeType.split(separator: "/").last.map(String.init) ?? mimeType
    let format = subtype.split(separator: "+").first.map { $0.uppercased() } ?? ""
    let size = ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle: .file)
    return bytes > 0 ? "\(format) · \(size)" : format
  }
}
