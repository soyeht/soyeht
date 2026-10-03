import Foundation
import UniformTypeIdentifiers

#if os(macOS)
  import AppKit
#elseif os(iOS)
  import UIKit
#endif

/// What a paste offers the composer. Copied files and copied images become
/// attachments; anything else is left for the text view to paste as text.
public struct AgentPasteboardContent: Sendable {
  public struct Image: Sendable {
    public let data: Data
    public let type: UTType

    public init(data: Data, type: UTType) {
      self.data = data
      self.type = type
    }
  }

  /// Image representations in preference order. A screenshot or an image copied
  /// from another app usually offers PNG and TIFF of the same picture; the first
  /// present one is used.
  public static let imageTypes: [UTType] = [.png, .jpeg, .heic, .gif, .webP, .tiff, .bmp]

  public var fileURLs: [URL]
  public var image: Image?
  public var hasText: Bool

  public init(fileURLs: [URL] = [], image: Image? = nil, hasText: Bool = false) {
    self.fileURLs = fileURLs
    self.image = image
    self.hasText = hasText
  }

  /// Copied files always attach: Finder also puts their names on the pasteboard
  /// as text. An image attaches unless the copy is text that carries a picture
  /// of itself, as a spreadsheet cell does.
  public var offersAttachments: Bool {
    !fileURLs.isEmpty || (image != nil && !hasText)
  }

  /// The drafts this paste adds, with a rejection for each item that cannot be read.
  public func drafts(linkHostFiles: Bool) -> [AgentAttachmentResult] {
    guard offersAttachments else { return [] }
    if !fileURLs.isEmpty {
      return fileURLs.map { url in
        Result { () throws(AgentAttachmentRejection) in
          try AgentAttachmentStaging.file(at: url, linkHostFiles: linkHostFiles)
        }
      }
    }
    guard let image else { return [] }
    guard let draft = AgentAttachmentStaging.image(image.data, type: image.type, name: nil) else {
      return [.failure(.unreadable(name: AgentAttachmentStaging.fallbackImageName))]
    }
    return [.success(draft)]
  }
}

#if os(macOS)
  extension AgentPasteboardContent {
    public init(pasteboard: NSPasteboard) {
      let files =
        pasteboard.readObjects(
          forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL]
      let image = Self.imageTypes.lazy.compactMap { type in
        pasteboard.data(forType: NSPasteboard.PasteboardType(type.identifier)).map {
          Image(data: $0, type: type)
        }
      }.first
      self.init(
        fileURLs: files ?? [], image: image,
        hasText: !(pasteboard.string(forType: .string) ?? "").isEmpty)
    }
  }
#elseif os(iOS)
  extension AgentPasteboardContent {
    /// Whether the pasteboard holds something to attach. It reads only the kinds
    /// of content, so it never triggers the system's paste permission prompt.
    @MainActor public static func hasAttachableContent(_ pasteboard: UIPasteboard) -> Bool {
      pasteboard.hasImages || pasteboard.hasURLs
    }

    /// Read only from the explicit Paste attachment action, so text is not offered.
    @MainActor public init(pasteboard: UIPasteboard) {
      let image = Self.imageTypes.lazy.compactMap { type in
        pasteboard.data(forPasteboardType: type.identifier).map { Image(data: $0, type: type) }
      }.first
      self.init(
        fileURLs: (pasteboard.urls ?? []).filter(\.isFileURL),
        image: image ?? pasteboard.image?.pngData().map { Image(data: $0, type: .png) },
        hasText: false)
    }
  }
#endif
