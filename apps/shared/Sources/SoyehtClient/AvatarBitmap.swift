import CoreGraphics
import Foundation
import ImageIO

/// A picture in memory: 8-bit RGBA with the colour already multiplied by the alpha, row after
/// row, top row first. The one pixel format of the avatar's rasteriser, so a layer blends with
/// plain arithmetic.
struct AvatarBitmap: Sendable {
  let width: Int
  let height: Int
  var pixels: [UInt8]

  private static var colorSpace: CGColorSpace {
    CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
  }

  init(width: Int, height: Int, pixels: [UInt8]? = nil) {
    self.width = width
    self.height = height
    self.pixels = pixels ?? [UInt8](repeating: 0, count: width * height * 4)
  }

  /// A picture drawn into a new bitmap of the given size (scaled, with the best filter the
  /// system has). `nil` when the system cannot make the context.
  private init?(drawing image: CGImage, width: Int, height: Int) {
    self.init(width: width, height: height)
    let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
      guard
        let context = CGContext(
          data: buffer.baseAddress, width: width, height: height, bitsPerComponent: 8,
          bytesPerRow: width * 4, space: Self.colorSpace,
          bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
      else { return false }
      context.interpolationQuality = .high
      context.setBlendMode(.copy)
      context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
      return true
    }
    if !drawn { return nil }
  }

  /// Decode a PNG (or any picture ImageIO reads) into this format. `nil` when it is not a
  /// picture, or is not the size the caller expects: a package declares its sizes, and a picture
  /// of another size is never drawn.
  init?(decoding data: Data, expecting size: AvatarSize) {
    let options = [kCGImageSourceShouldCache: false] as CFDictionary
    guard let source = CGImageSourceCreateWithData(data as CFData, options),
      let image = CGImageSourceCreateImageAtIndex(source, 0, options),
      image.width == Int(size.width), image.height == Int(size.height)
    else { return nil }
    self.init(drawing: image, width: image.width, height: image.height)
  }

  /// The picture at another size, averaged down or up (itself when the size is the same, and
  /// when the system cannot scale it).
  func resized(width newWidth: Int, height newHeight: Int) -> AvatarBitmap {
    guard newWidth != width || newHeight != height, let image = cgImage else { return self }
    return AvatarBitmap(drawing: image, width: newWidth, height: newHeight) ?? self
  }

  /// This picture with another laid over it, source-over, its top left corner at `(x, y)`.
  func laying(_ other: AvatarBitmap, atX x: Int, y: Int) -> AvatarBitmap {
    var out = self
    out.pixels.withUnsafeMutableBufferPointer { to in
      other.pixels.withUnsafeBufferPointer { from in
        guard let to = to.baseAddress, let from = from.baseAddress else { return }
        for row in 0..<other.height where (0..<height).contains(y + row) {
          for column in 0..<other.width where (0..<width).contains(x + column) {
            let s = row * other.width + column
            let d = (y + row) * width + x + column
            let keep = 1 - AvatarRasterizer.load(from, s)[3] / 255
            AvatarRasterizer.store(
              AvatarRasterizer.load(from, s) + AvatarRasterizer.load(to, d) * keep, to, d)
          }
        }
      }
    }
    return out
  }

  /// The pixels of a rectangle of this picture.
  func cropped(_ rect: AvatarPixelRect) -> AvatarBitmap {
    var out = AvatarBitmap(width: max(rect.x1 - rect.x0, 0), height: max(rect.y1 - rect.y0, 0))
    for row in 0..<out.height {
      let from = ((rect.y0 + row) * width + rect.x0) * 4
      out.pixels.replaceSubrange(
        row * out.width * 4..<(row + 1) * out.width * 4, with: pixels[from..<from + out.width * 4])
    }
    return out
  }

  /// The picture as a `CGImage` the system draws (a copy: the bitmap goes on being drawn into).
  var cgImage: CGImage? {
    guard let provider = CGDataProvider(data: Data(pixels) as CFData) else { return nil }
    return CGImage(
      width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32,
      bytesPerRow: width * 4, space: Self.colorSpace,
      bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedLast.rawValue),
      provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent)
  }
}
