import Foundation

/// The software rasteriser's two primitives, on premultiplied RGBA bitmaps: a textured triangle
/// blended over a bitmap, and a bilinear sample. One core, no GPU: the reference implementation
/// ADR 0166 asks for, so the three renderers draw the same picture from the same parameters.
enum AvatarRasterizer {
  /// A vertex of a textured triangle: a position in the destination's pixels, a position in the
  /// texture's pixels, and the opacity of the vertex.
  struct Vertex {
    var x: Double
    var y: Double
    var u: Double
    var v: Double
    var alpha: Double
  }

  /// The four channels of the pixel at `index` (a pixel, not a byte), as 0...255 numbers. The
  /// channels are the four bytes of a little-endian word, read at once.
  @inline(__always)
  static func load(_ pixels: UnsafePointer<UInt8>, _ index: Int) -> SIMD4<Float> {
    let word = UnsafeRawPointer(pixels).loadUnaligned(fromByteOffset: index * 4, as: UInt32.self)
    return SIMD4<Float>(
      Float(word & 255), Float((word >> 8) & 255), Float((word >> 16) & 255), Float(word >> 24))
  }

  /// Store four channels (0...255, rounded to the nearest) as the pixel at `index`.
  @inline(__always)
  static func store(_ value: SIMD4<Float>, _ pixels: UnsafeMutablePointer<UInt8>, _ index: Int) {
    let word =
      UInt32(clamp8(value[0])) | UInt32(clamp8(value[1])) << 8 | UInt32(clamp8(value[2])) << 16
      | UInt32(clamp8(value[3])) << 24
    UnsafeMutableRawPointer(pixels).storeBytes(of: word, toByteOffset: index * 4, as: UInt32.self)
  }

  /// Bilinear sample of a texture at the continuous position `(u, v)` of its pixels (a pixel
  /// covers a unit square, so its centre is at +0.5), clamped to the edge.
  @inline(__always)
  static func sample(
    _ texture: UnsafePointer<UInt8>, width: Int, height: Int, u: Float, v: Float
  ) -> SIMD4<Float> {
    let x = min(max(u - 0.5, 0), Float(width - 1))
    let y = min(max(v - 0.5, 0), Float(height - 1))
    let x0 = Int(x)
    let y0 = Int(y)
    let fx = x - Float(x0)
    let fy = y - Float(y0)
    let at = y0 * width + x0
    let right = x0 + 1 < width ? 1 : 0
    let below = y0 + 1 < height ? width : 0
    let top = load(texture, at) * (1 - fx) + load(texture, at + right) * fx
    let bottom = load(texture, at + below) * (1 - fx) + load(texture, at + below + right) * fx
    return top * (1 - fy) + bottom * fy
  }

  /// Whether an edge is a top or left edge of a triangle wound so that the interior is on the
  /// positive side: a pixel centre exactly on such an edge belongs to the triangle, and on any
  /// other edge to its neighbour, so two triangles that share an edge never draw a pixel twice.
  @inline(__always)
  private static func owns(dx: Double, dy: Double) -> Bool { dy < 0 || (dy == 0 && dx > 0) }

  /// Draw one textured triangle over `destination`, `opacity` times the interpolated vertex
  /// opacity (source-over, premultiplied). A triangle with no area, or with a vertex that is not
  /// a number, draws nothing.
  static func fill(
    _ a: Vertex, _ b: Vertex, _ c: Vertex, opacity: Double, texture: UnsafePointer<UInt8>,
    textureSize: (width: Int, height: Int), destination: UnsafeMutablePointer<UInt8>,
    destinationSize: (width: Int, height: Int)
  ) {
    var (v0, v1, v2) = (a, b, c)
    var area = (v1.x - v0.x) * (v2.y - v0.y) - (v1.y - v0.y) * (v2.x - v0.x)
    guard area.isFinite, abs(area) > 1e-9 else { return }
    if area < 0 {
      swap(&v1, &v2)
      area = -area
    }
    // The pixels whose centres the triangle's box covers, bounded in doubles first: a position a
    // host got wrong must not be an integer overflow.
    let lowX = max((min(v0.x, v1.x, v2.x) - 0.5).rounded(.up), 0)
    let highX = min((max(v0.x, v1.x, v2.x) - 0.5).rounded(.down), Double(destinationSize.width - 1))
    let lowY = max((min(v0.y, v1.y, v2.y) - 0.5).rounded(.up), 0)
    let highY = min(
      (max(v0.y, v1.y, v2.y) - 0.5).rounded(.down), Double(destinationSize.height - 1))
    guard lowX <= highX, lowY <= highY else { return }
    let (minX, maxX, minY, maxY) = (Int(lowX), Int(highX), Int(lowY), Int(highY))
    // The edge opposite each vertex, and whether this triangle owns a pixel centre on it.
    let own0 = owns(dx: v2.x - v1.x, dy: v2.y - v1.y)
    let own1 = owns(dx: v0.x - v2.x, dy: v0.y - v2.y)
    let own2 = owns(dx: v1.x - v0.x, dy: v1.y - v0.y)
    for y in minY...maxY {
      let py = Double(y) + 0.5
      for x in minX...maxX {
        let px = Double(x) + 0.5
        let w0 = (v2.x - v1.x) * (py - v1.y) - (v2.y - v1.y) * (px - v1.x)
        let w1 = (v0.x - v2.x) * (py - v2.y) - (v0.y - v2.y) * (px - v2.x)
        let w2 = (v1.x - v0.x) * (py - v0.y) - (v1.y - v0.y) * (px - v0.x)
        guard w0 > 0 || (w0 == 0 && own0), w1 > 0 || (w1 == 0 && own1),
          w2 > 0 || (w2 == 0 && own2)
        else { continue }
        let b0 = w0 / area
        let b1 = w1 / area
        let b2 = w2 / area
        let coverage = opacity * (b0 * v0.alpha + b1 * v1.alpha + b2 * v2.alpha)
        guard coverage > 0 else { continue }
        let color = sample(
          texture, width: textureSize.width, height: textureSize.height,
          u: Float(b0 * v0.u + b1 * v1.u + b2 * v2.u), v: Float(b0 * v0.v + b1 * v1.v + b2 * v2.v))
        let index = y * destinationSize.width + x
        let k = Float(min(coverage, 1))
        store(color * k + load(destination, index) * (1 - k), destination, index)
      }
    }
  }

  @inline(__always)
  static func clamp8(_ x: Float) -> UInt8 { UInt8(min(max(x + 0.5, 0), 255)) }
}
