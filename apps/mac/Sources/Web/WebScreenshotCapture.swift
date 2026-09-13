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

  private static func boundedPNG(_ image: NSImage) throws -> Data {
    var candidate = image
    for _ in 0..<6 {
      guard let tiff = candidate.tiffRepresentation,
        let bitmap = NSBitmapImageRep(data: tiff),
        let png = bitmap.representation(using: .png, properties: [:])
      else {
        throw ClientFailure.webHost(.internal_error, "The page snapshot could not be encoded.")
      }
      if png.count <= ProtocolLimits.webScreenshotBytes { return png }
      let ratio = sqrt(Double(ProtocolLimits.webScreenshotBytes) / Double(png.count)) * 0.85
      candidate = try resized(candidate, ratio: min(0.9, ratio))
    }
    throw ClientFailure.webHost(.internal_error, "The page snapshot exceeds its byte limit.")
  }

  private static func resized(_ image: NSImage, ratio: Double) throws -> NSImage {
    let width = max(1, Int(floor(image.size.width * ratio)))
    let height = max(1, Int(floor(image.size.height * ratio)))
    guard
      let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0)
    else { throw ClientFailure.webHost(.internal_error, "The page snapshot could not be resized.") }
    NSGraphicsContext.saveGraphicsState()
    defer { NSGraphicsContext.restoreGraphicsState() }
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
    image.draw(
      in: NSRect(x: 0, y: 0, width: width, height: height), from: .zero,
      operation: .copy, fraction: 1)
    let resized = NSImage(size: NSSize(width: width, height: height))
    resized.addRepresentation(bitmap)
    return resized
  }
}
