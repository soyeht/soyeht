import AppKit
import SoyehtClient
import SoyehtProtocol
import WebKit

@MainActor enum WebScreenshotCapture {
  private static let pixelBudget = 4_000_000.0

  static func capture(webView: WKWebView, fullPage: Bool) async throws -> WebHostAttachment {
    let configuration = try await configuration(for: webView, fullPage: fullPage)
    let image = try await snapshot(webView, configuration: configuration)
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
    let image = try await snapshot(webView, configuration: configuration)
    let bytes = try boundedPNG(image, maxBytes: 30 * 1_024)
    let id = UUID().uuidString.lowercased()
    return WebHostAttachment(
      reference: AttachmentRef(
        attachmentId: id, mimeType: "image/png", name: "selected-element.png",
        bytes: String(bytes.count)),
      bytes: bytes)
  }

  private static func configuration(
    for webView: WKWebView, fullPage: Bool
  ) async throws -> WKSnapshotConfiguration {
    let viewport = webView.bounds
    let content = fullPage ? try await documentRect(webView) : viewport
    let rect = fullPage ? content : viewport
    guard rect.width > 0, rect.height > 0 else {
      throw ClientFailure.webHost(.internal_error, "The web viewport is unavailable.")
    }
    let configuration = WKSnapshotConfiguration()
    configuration.rect = rect
    let pixels = rect.width * rect.height
    if pixels > pixelBudget {
      configuration.snapshotWidth = NSNumber(
        value: max(1, floor(rect.width * sqrt(pixelBudget / pixels))))
    }
    return configuration
  }

  private static func documentRect(_ webView: WKWebView) async throws -> CGRect {
    let raw = try await webView.callAsyncJavaScript(
      "return {width: Math.max(document.documentElement.scrollWidth, document.body?.scrollWidth || 0), height: Math.max(document.documentElement.scrollHeight, document.body?.scrollHeight || 0)};",
      arguments: [:], in: nil, contentWorld: .page)
    guard let value = raw as? [String: Any], let width = value["width"] as? NSNumber,
      let height = value["height"] as? NSNumber, width.doubleValue > 0, height.doubleValue > 0
    else { throw ClientFailure.webHost(.internal_error, "The page dimensions are unavailable.") }
    return CGRect(x: 0, y: 0, width: width.doubleValue, height: height.doubleValue)
  }

  private static func snapshot(
    _ webView: WKWebView, configuration: WKSnapshotConfiguration
  ) async throws -> NSImage {
    try await withCheckedThrowingContinuation { continuation in
      webView.takeSnapshot(with: configuration) { image, error in
        if let image {
          continuation.resume(returning: image)
        } else {
          continuation.resume(
            throwing: error
              ?? ClientFailure.webHost(.internal_error, "The page snapshot failed."))
        }
      }
    }
  }

  private static func boundedPNG(
    _ image: NSImage, maxBytes: Int = ProtocolLimits.webScreenshotBytes
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
