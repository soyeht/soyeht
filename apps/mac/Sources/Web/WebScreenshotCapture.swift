import AppKit
import SoyehtClient
import SoyehtProtocol
import WebKit

@MainActor enum WebScreenshotCapture {
  /// The pane's visible area.
  static func capture(webView: WKWebView) async throws -> WebHostAttachment {
    let viewport = webView.bounds
    guard viewport.width > 0, viewport.height > 0 else {
      throw ClientFailure.webHost(.internal_error, "The web viewport is unavailable.")
    }
    let configuration = WKSnapshotConfiguration()
    configuration.rect = viewport
    let pixels = viewport.width * viewport.height
    if pixels > WebPageReader.pixelBudget {
      configuration.snapshotWidth = NSNumber(
        value: max(1, floor(viewport.width * sqrt(WebPageReader.pixelBudget / pixels))))
    }
    return try attachment(
      for: try await WebPageReader.snapshot(webView, configuration: configuration))
  }

  /// The page as the reader walks it (ADR 0109): at most `webReadScreens` screens, so the caller
  /// must say whether the page ended there.
  static func captureFullPage(
    reader: WebPageReader, deadline: ContinuousClock.Instant, between: @escaping () throws -> Void
  ) async throws -> (attachment: WebHostAttachment, endOfPage: Bool) {
    let capture = try await reader.read(
      screens: ProtocolLimits.webReadScreens, collectsContent: false, deadline: deadline,
      between: between)
    return (try attachment(for: capture.image), capture.endOfPage)
  }

  static func attachment(for image: NSImage) throws -> WebHostAttachment {
    let bytes = try boundedPNG(image)
    let id = UUID().uuidString.lowercased()
    let reference = AttachmentRef(
      attachmentId: id, mimeType: "image/png", name: "screenshot.png",
      bytes: String(bytes.count))
    return WebHostAttachment(reference: reference, bytes: bytes)
  }

  static func capture(webView: WKWebView, viewportRect: WebRect) async throws -> WebHostAttachment {
    let requested = CGRect(
      x: CGFloat(viewportRect.x) - 16, y: CGFloat(viewportRect.y) - 16,
      width: CGFloat(viewportRect.width) + 32, height: CGFloat(viewportRect.height) + 32)
    let rect = requested.intersection(webView.bounds)
    guard !rect.isNull, rect.width > 0, rect.height > 0 else {
      throw ClientFailure.webHost(.internal_error, "The selected element is outside the viewport.")
    }
    let configuration = WKSnapshotConfiguration()
    configuration.rect = rect
    if rect.width > CGFloat(ProtocolLimits.webPickCropWidth) {
      configuration.snapshotWidth = NSNumber(value: ProtocolLimits.webPickCropWidth)
    }
    let image = try await WebPageReader.snapshot(webView, configuration: configuration)
    let bytes = try boundedPNG(image, maxBytes: 30 * 1_024)
    let id = UUID().uuidString.lowercased()
    return WebHostAttachment(
      reference: AttachmentRef(
        attachmentId: id, mimeType: "image/png", name: "selected-element.png",
        bytes: String(bytes.count)),
      bytes: bytes)
  }

  private static func boundedPNG(
    _ image: NSImage, maxBytes: Int = ProtocolLimits.webAgentImageBytes
  ) throws -> Data {
    try image.boundedPNG(
      maxBytes: maxBytes,
      failures: BoundedPNGFailures(
        encoding: ClientFailure.webHost(
          .internal_error, "The page snapshot could not be encoded."),
        resizing: ClientFailure.webHost(
          .internal_error, "The page snapshot could not be resized."),
        limit: ClientFailure.webHost(
          .internal_error, "The page snapshot exceeds its byte limit.")))
  }
}
