import Foundation
import ImageIO
import SoyehtProtocol
import UniformTypeIdentifiers

/// Turns picked, dropped and pasted content into attachment drafts. Images are
/// prepared so the agent can see them: a type the agents do not accept inline,
/// such as TIFF or HEIC, or an image too large for one ACP message, is re-encoded
/// as PNG or JPEG. Other files are linked on the engine's Mac, or sent as bytes
/// from a paired device.
public enum AgentAttachmentStaging {
  /// Image types every pinned agent takes inline (claude-agent-acp and codex-acp).
  static let inlineImageTypes: [UTType] = [.png, .jpeg, .gif, .webP]
  /// Providers downscale larger images anyway, and the limit keeps an image
  /// inside the one-line ACP message once encoded as base64.
  static let maximumImagePixels = 2_048
  static let maximumImageBytes = 700 * 1_024
  static let thumbnailPixels = 160
  static let fallbackImageName = "Pasted image"
  /// The longest sides an image is scaled to, the largest that fits first.
  static let scalePixels = [maximumImagePixels, 1_600, 1_280, 1_024, 768]
  /// The JPEG qualities tried at each scale, the best first.
  static let jpegQualities = [0.85, 0.7, 0.55, 0.4]
  /// The preview's longest side and JPEG quality, tried in turn until one fits the budget.
  static let thumbnailAttempts = [(thumbnailPixels, 0.7), (thumbnailPixels, 0.45), (96, 0.45)]

  /// Whether an image goes to the agent as it is: a type every agent takes inline, small enough
  /// in bytes and in pixels.
  static func keepsOriginal(type: UTType, byteCount: Int, width: Int, height: Int) -> Bool {
    inlineImageTypes.contains(type) && byteCount <= maximumImageBytes
      && max(width, height) <= maximumImagePixels
  }

  /// The name an image is attached under: its own name without the extension, or
  /// "Pasted image", with the extension of the type it is sent as.
  static func imageFileName(_ name: String?, type: UTType) -> String {
    let base = (name.map { ($0 as NSString).deletingPathExtension } ?? "").trimmingCharacters(
      in: .whitespacesAndNewlines)
    return "\(base.isEmpty ? fallbackImageName : base).\(type.preferredFilenameExtension ?? "png")"
  }

  public static func kind(of type: UTType) -> PromptAttachmentKind {
    if type.conforms(to: .image) { return .image }
    if type.conforms(to: .audio) { return .audio }
    if type.conforms(to: .text) { return .text }
    return .file
  }

  static func mimeType(of type: UTType, kind: PromptAttachmentKind) -> String {
    type.preferredMIMEType ?? (kind == .text ? "text/plain" : "application/octet-stream")
  }

  /// A file picked or dropped. Images are always sent as prepared bytes; another
  /// file is linked in place when this client runs on the engine's Mac.
  public static func file(at url: URL, linkHostFiles: Bool) throws(AgentAttachmentRejection)
    -> AgentAttachmentDraft
  {
    let name = url.lastPathComponent
    let scoped = url.startAccessingSecurityScopedResource()
    defer { if scoped { url.stopAccessingSecurityScopedResource() } }
    let values = try? url.resourceValues(forKeys: [.isDirectoryKey, .fileSizeKey, .contentTypeKey])
    guard let values else { throw .unreadable(name: name) }
    if values.isDirectory == true { throw .folder(name: name) }
    let type = values.contentType ?? UTType(filenameExtension: url.pathExtension) ?? .data
    let size = values.fileSize ?? 0
    let kind = kind(of: type)
    if kind == .image, size <= ProtocolLimits.promptAttachmentBytes,
      let data = try? Data(contentsOf: url), let draft = image(data, type: type, name: name)
    {
      return draft
    }
    if linkHostFiles {
      return AgentAttachmentDraft(
        kind: kind, name: name, mimeType: mimeType(of: type, kind: kind), byteCount: size,
        source: .hostFile(url.standardizedFileURL.path))
    }
    guard size <= ProtocolLimits.promptAttachmentBytes else { throw .tooLarge(name: name) }
    guard let data = try? Data(contentsOf: url) else { throw .unreadable(name: name) }
    return bytes(data, name: name, type: type)
  }

  /// Bytes with a known type, such as a file a device app provides by value.
  public static func bytes(_ data: Data, name: String, type: UTType) -> AgentAttachmentDraft {
    let kind = kind(of: type)
    if kind == .image, let draft = image(data, type: type, name: name) { return draft }
    return AgentAttachmentDraft(
      kind: kind, name: name, mimeType: mimeType(of: type, kind: kind), byteCount: data.count,
      source: .bytes(data))
  }

  /// An image from the pasteboard, a drop or the photo library, prepared for the
  /// agent with a thumbnail. Returns nil when the bytes are not a readable image.
  public static func image(_ data: Data, type: UTType, name: String?) -> AgentAttachmentDraft? {
    guard let source = CGImageSourceCreateWithData(data as CFData, nil),
      CGImageSourceGetCount(source) > 0
    else { return nil }
    let (prepared, preparedType) = preparedImage(data, type: type, source: source)
    guard let prepared else { return nil }
    return AgentAttachmentDraft(
      kind: .image, name: imageFileName(name, type: preparedType),
      mimeType: mimeType(of: preparedType, kind: .image),
      byteCount: prepared.count, source: .bytes(prepared), thumbnail: thumbnail(source))
  }

  private static func preparedImage(_ data: Data, type: UTType, source: CGImageSource) -> (
    Data?, UTType
  ) {
    let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any]
    let width = properties?[kCGImagePropertyPixelWidth] as? Int ?? 0
    let height = properties?[kCGImagePropertyPixelHeight] as? Int ?? 0
    if keepsOriginal(type: type, byteCount: data.count, width: width, height: height) {
      return (data, type)
    }
    var smallest: Data?
    for pixels in scalePixels {
      guard let image = scaled(source, maximumPixels: pixels) else { return (nil, type) }
      let opaque = [.none, .noneSkipFirst, .noneSkipLast].contains(image.alphaInfo)
      if !opaque, let png = encoded(image, as: .png), png.count <= maximumImageBytes {
        return (png, .png)
      }
      for quality in jpegQualities {
        let jpeg = encoded(image, as: .jpeg, quality: quality)
        if let jpeg, jpeg.count <= maximumImageBytes { return (jpeg, .jpeg) }
        smallest = jpeg ?? smallest
      }
    }
    // Still large: the engine links it instead of sending it inline.
    return (smallest, .jpeg)
  }

  /// A preview within the protocol's thumbnail budget, or nil.
  static func thumbnail(_ source: CGImageSource) -> Data? {
    for (pixels, quality) in thumbnailAttempts {
      if let image = scaled(source, maximumPixels: pixels),
        let jpeg = encoded(image, as: .jpeg, quality: quality),
        jpeg.count <= ProtocolLimits.promptAttachmentThumbnailBytes
      {
        return jpeg
      }
    }
    return nil
  }

  private static func scaled(_ source: CGImageSource, maximumPixels: Int) -> CGImage? {
    let options: [CFString: Any] = [
      kCGImageSourceCreateThumbnailFromImageAlways: true,
      kCGImageSourceCreateThumbnailWithTransform: true,
      kCGImageSourceThumbnailMaxPixelSize: maximumPixels,
    ]
    return CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary)
  }

  private static func encoded(_ image: CGImage, as type: UTType, quality: Double = 1) -> Data? {
    let output = NSMutableData()
    guard
      let destination = CGImageDestinationCreateWithData(
        output, type.identifier as CFString, 1, nil)
    else { return nil }
    let options = [kCGImageDestinationLossyCompressionQuality: quality] as CFDictionary
    CGImageDestinationAddImage(destination, image, options)
    return CGImageDestinationFinalize(destination) ? output as Data : nil
  }
}
