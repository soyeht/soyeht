import Foundation
import SoyehtProtocol

/// The picture on an attachment chip that has no thumbnail. Each client draws its own icon for it.
enum AgentAttachmentGlyph: String, Sendable {
  case image, audio, text, pdf, file

  init(kind: PromptAttachmentKind, mimeType: String) {
    switch kind {
    case .image: self = .image
    case .audio: self = .audio
    case .text: self = .text
    case .file: self = mimeType == "application/pdf" ? .pdf : .file
    }
  }

  var systemImage: String {
    switch self {
    case .image: "photo"
    case .audio: "waveform"
    case .text: "doc.text"
    case .pdf: "doc.richtext"
    case .file: "doc"
    }
  }
}

/// How one attachment chip reads, in the composer and in a sent message.
struct AgentAttachmentPresentation: Identifiable, Equatable, Sendable {
  let id: String
  let name: String
  let glyph: AgentAttachmentGlyph
  var systemImage: String { glyph.systemImage }
  let detail: String
  let thumbnail: Data?

  init(draft: AgentAttachmentDraft) {
    id = draft.id.uuidString
    name = draft.name
    glyph = AgentAttachmentGlyph(kind: draft.kind, mimeType: draft.mimeType)
    detail = Self.detail(mimeType: draft.mimeType, bytes: draft.byteCount)
    thumbnail = draft.thumbnail
  }

  /// A sent attachment; the transcript keeps its name, size and thumbnail.
  init(attachment: PromptAttachment, index: Int) {
    id = "\(index)-\(attachment.path ?? attachment.name)"
    name = attachment.name
    glyph = AgentAttachmentGlyph(kind: attachment.kind, mimeType: attachment.mimeType)
    detail = Self.detail(mimeType: attachment.mimeType, bytes: Int(attachment.bytes) ?? 0)
    thumbnail = attachment.thumbnail.flatMap { Data(base64Encoded: $0) }
  }

  static func systemImage(kind: PromptAttachmentKind, mimeType: String) -> String {
    AgentAttachmentGlyph(kind: kind, mimeType: mimeType).systemImage
  }

  /// "PNG · 12 KB": the subtype of the MIME type and a file-style size.
  static func detail(mimeType: String, bytes: Int, locale: Locale = .current) -> String {
    let subtype = mimeType.split(separator: "/").last.map(String.init) ?? mimeType
    let format = subtype.split(separator: "+").first.map { $0.uppercased() } ?? ""
    return bytes > 0 ? "\(format) · \(fileSize(bytes, locale: locale))" : format
  }

  /// A size as Finder writes it: decimal units, no decimals for KB, one for MB, two above, a
  /// trailing zero dropped and a half rounded up. `ByteCountFormatter` writes the same, but only
  /// in the current locale; the desktop client has no such formatter, so the rule is written out
  /// here and both clients pass `contracts/vectors/attachments.json`.
  static func fileSize(_ bytes: Int, locale: Locale = .current) -> String {
    guard bytes >= 1_000 else { return "\(bytes) \(bytes == 1 ? "byte" : "bytes")" }
    let units = ["KB", "MB", "GB", "TB", "PB", "EB"]
    let decimals = [0, 1, 2, 2, 2, 2]
    var unit = 0
    var scale = 1_000
    while bytes / scale >= 1_000 && unit < units.count - 1 {
      scale *= 1_000
      unit += 1
    }
    var factor = 1
    for _ in 0..<decimals[unit] { factor *= 10 }
    var scaled = (bytes * factor * 2 + scale) / (2 * scale)
    // A value that rounds up to 1000 is one of the next unit.
    if scaled >= 1_000 * factor && unit < units.count - 1 {
      unit += 1
      factor = 1
      scaled = 1
    }
    var text = "\(scaled / factor)"
    var fraction = String(scaled % factor)
    if factor > 1 {
      fraction = String(repeating: "0", count: "\(factor)".count - 1 - fraction.count) + fraction
      while fraction.hasSuffix("0") { fraction.removeLast() }
      if !fraction.isEmpty { text += (locale.decimalSeparator ?? ".") + fraction }
    }
    return "\(text) \(units[unit])"
  }
}
