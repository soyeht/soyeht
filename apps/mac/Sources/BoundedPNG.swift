import AppKit

/// The errors a PNG that cannot be made small enough ends in; each caller names them in its own
/// words.
struct BoundedPNGFailures {
  let encoding: Error
  let resizing: Error
  let limit: Error
}

extension NSImage {
  /// The image as a PNG of at most `maxBytes`, shrunk step by step until it fits.
  func boundedPNG(maxBytes: Int, failures: BoundedPNGFailures) throws -> Data {
    var candidate = self
    for _ in 0..<8 {
      guard let tiff = candidate.tiffRepresentation,
        let bitmap = NSBitmapImageRep(data: tiff),
        let png = bitmap.representation(using: .png, properties: [:])
      else { throw failures.encoding }
      if png.count <= maxBytes { return png }
      let ratio = sqrt(Double(maxBytes) / Double(png.count)) * 0.85
      candidate = try candidate.resized(ratio: min(0.9, ratio), failure: failures.resizing)
    }
    throw failures.limit
  }

  private func resized(ratio: Double, failure: Error) throws -> NSImage {
    let width = max(1, Int(floor(size.width * ratio)))
    let height = max(1, Int(floor(size.height * ratio)))
    guard
      let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: width, pixelsHigh: height, bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0)
    else { throw failure }
    NSGraphicsContext.saveGraphicsState()
    defer { NSGraphicsContext.restoreGraphicsState() }
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
    draw(
      in: NSRect(x: 0, y: 0, width: width, height: height), from: .zero,
      operation: .copy, fraction: 1)
    let resized = NSImage(size: NSSize(width: width, height: height))
    resized.addRepresentation(bitmap)
    return resized
  }
}
