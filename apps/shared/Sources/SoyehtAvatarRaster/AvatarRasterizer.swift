import Foundation

/// The software rasteriser's two primitives, on premultiplied RGBA bitmaps: a textured triangle
/// blended over a bitmap, and a bilinear sample. One core, no GPU: the reference implementation
/// ADR 0166 asks for, so the three renderers draw the same picture from the same parameters.
package enum AvatarRasterizer {
  /// A vertex of a textured triangle: a position in the destination's pixels, a position in the
  /// texture's pixels, and the opacity of the vertex.
  package struct Vertex {
    package var x: Double
    package var y: Double
    package var u: Double
    package var v: Double
    package var alpha: Double

    package init(x: Double, y: Double, u: Double, v: Double, alpha: Double) {
      self.x = x
      self.y = y
      self.u = u
      self.v = v
      self.alpha = alpha
    }
  }

  /// The four channels of the pixel at `index` (a pixel, not a byte), as 0...255 numbers. The
  /// channels are the four bytes of a little-endian word, read at once.
  @inline(__always)
  package static func load(_ pixels: UnsafePointer<UInt8>, _ index: Int) -> SIMD4<Float> {
    let word = UnsafeRawPointer(pixels).loadUnaligned(fromByteOffset: index * 4, as: UInt32.self)
    return SIMD4<Float>(
      Float(word & 255), Float((word >> 8) & 255), Float((word >> 16) & 255), Float(word >> 24))
  }

  /// Store four channels (0...255, rounded to the nearest) as the pixel at `index`.
  @inline(__always)
  package static func store(
    _ value: SIMD4<Float>, _ pixels: UnsafeMutablePointer<UInt8>, _ index: Int
  ) {
    let word =
      UInt32(clamp8(value[0])) | UInt32(clamp8(value[1])) << 8 | UInt32(clamp8(value[2])) << 16
      | UInt32(clamp8(value[3])) << 24
    UnsafeMutableRawPointer(pixels).storeBytes(of: word, toByteOffset: index * 4, as: UInt32.self)
  }

  /// Bilinear sample of a texture at the continuous position `(u, v)` of its pixels (a pixel
  /// covers a unit square, so its centre is at +0.5), clamped to the edge.
  @inline(__always)
  package static func sample(
    _ texture: UnsafePointer<UInt8>, width: Int, height: Int, u: Float, v: Float
  ) -> SIMD4<Float> {
    // A position that is not a number reads the first pixel: `Int(_:)` of one would trap.
    let x = Float.minimum(Float.maximum(u - 0.5, 0), Float(width - 1))
    let y = Float.minimum(Float.maximum(v - 0.5, 0), Float(height - 1))
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
  /// opacity (source-over, premultiplied), and say how many pixels it covered. A triangle with no
  /// area, or with a vertex that is not a number, draws nothing.
  ///
  /// The cost is that of the pixels covered: each row is cut to the span the three edges leave,
  /// and only the pixels of the span (and one on each side, for the rounding of the cut) are
  /// tested.
  ///
  /// With `under` (the picture as it was before the part that this triangle belongs to), the
  /// vertices' opacity is the coverage of the whole part, applied once to what was under it, and
  /// `opacity` mixes the layers: see `AvatarMeshPart.layers`.
  @discardableResult
  package static func fill(
    _ a: Vertex, _ b: Vertex, _ c: Vertex, opacity: Double, under: UnsafePointer<UInt8>? = nil,
    texture: UnsafePointer<UInt8>, textureSize: (width: Int, height: Int),
    destination: UnsafeMutablePointer<UInt8>, destinationSize: (width: Int, height: Int)
  ) -> Int {
    var (v0, v1, v2) = (a, b, c)
    var area = (v1.x - v0.x) * (v2.y - v0.y) - (v1.y - v0.y) * (v2.x - v0.x)
    guard area.isFinite, abs(area) > 1e-9 else { return 0 }
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
    guard lowX <= highX, lowY <= highY else { return 0 }
    let (minX, maxX, minY, maxY) = (Int(lowX), Int(highX), Int(lowY), Int(highY))
    // The edge opposite each vertex, and whether this triangle owns a pixel centre on it.
    let own0 = owns(dx: v2.x - v1.x, dy: v2.y - v1.y)
    let own1 = owns(dx: v0.x - v2.x, dy: v0.y - v2.y)
    let own2 = owns(dx: v1.x - v0.x, dy: v1.y - v0.y)
    var covered = 0
    for y in minY...maxY {
      let py = Double(y) + 0.5
      // Where the three edge functions are all positive, along this row: each is a line in the
      // pixel's x, so the row's span is an interval.
      let span = Self.span(row: py, v0, v1, v2, from: minX, to: maxX)
      guard span.lowerBound <= span.upperBound else { continue }
      for x in span {
        let px = Double(x) + 0.5
        let w0 = (v2.x - v1.x) * (py - v1.y) - (v2.y - v1.y) * (px - v1.x)
        let w1 = (v0.x - v2.x) * (py - v2.y) - (v0.y - v2.y) * (px - v2.x)
        let w2 = (v1.x - v0.x) * (py - v0.y) - (v1.y - v0.y) * (px - v0.x)
        guard w0 > 0 || (w0 == 0 && own0), w1 > 0 || (w1 == 0 && own1),
          w2 > 0 || (w2 == 0 && own2)
        else { continue }
        covered += 1
        let b0 = w0 / area
        let b1 = w1 / area
        let b2 = w2 / area
        let coverage = opacity * (b0 * v0.alpha + b1 * v1.alpha + b2 * v2.alpha)
        guard coverage > 0 || under != nil else { continue }
        let color = sample(
          texture, width: textureSize.width, height: textureSize.height,
          u: Float(b0 * v0.u + b1 * v1.u + b2 * v2.u), v: Float(b0 * v0.v + b1 * v1.v + b2 * v2.v))
        let index = y * destinationSize.width + x
        if let under {
          // The vertices' opacity (the feather of an edge) is the coverage of the whole mouth
          // part, applied once to the picture that was under it; the layer's opacity then mixes
          // the layers by their weights. Applied to each layer, the coverage of an edge would
          // add up with every layer drawn, and a layer that comes in at a tiny weight would
          // change the edge.
          let a = Float(min(max(b0 * v0.alpha + b1 * v1.alpha + b2 * v2.alpha, 0), 1))
          let effective = color * a + load(under, index) * (1 - a * color[3] / 255)
          let mix = Float(min(opacity, 1))
          store(effective * mix + load(destination, index) * (1 - mix), destination, index)
          continue
        }
        // Source-over of a premultiplied texel: the texel's own alpha, scaled by the coverage,
        // is what the destination keeps less of.
        let k = Float(min(coverage, 1))
        store(
          color * k + load(destination, index) * (1 - k * color[3] / 255), destination, index)
      }
    }
    return covered
  }

  /// The pixels of the row `py` (a pixel centre's y) that can be inside the triangle: the
  /// interval its three edge functions leave, one pixel wider on each side for rounding, cut to
  /// `from...to`. Empty (a lower bound over the upper) when the row misses the triangle.
  private static func span(
    row py: Double, _ v0: Vertex, _ v1: Vertex, _ v2: Vertex, from: Int, to: Int
  ) -> ClosedRange<Int> {
    var low = -Double.infinity
    var high = Double.infinity
    // The edge function along the row is `slope * px + offset`, and it must not be negative.
    // Returns false when no pixel of the row can satisfy it.
    func cut(_ p: Vertex, _ q: Vertex) -> Bool {
      let slope = -(q.y - p.y)
      let offset = (q.x - p.x) * (py - p.y) + (q.y - p.y) * p.x
      if slope > 0 {
        low = max(low, -offset / slope)
      } else if slope < 0 {
        high = min(high, -offset / slope)
      } else if offset < 0 {
        return false
      }
      return true
    }
    guard cut(v1, v2), cut(v2, v0), cut(v0, v1) else { return 1...0 }
    // Pixel x has its centre at x + 0.5.
    let first = max((low - 0.5).rounded(.up) - 1, Double(from))
    let last = min((high - 0.5).rounded(.down) + 1, Double(to))
    guard first <= last else { return 1...0 }
    return Int(first)...Int(last)
  }

  @inline(__always)
  package static func clamp8(_ x: Float) -> UInt8 { UInt8(min(max(x + 0.5, 0), 255)) }

  /// The warp: for every output pixel, read `picture` bilinearly at the position interpolated
  /// across the cell of the grid it is in. `gridX` and `gridY` hold, for the vertex at every
  /// `grid.cell` output pixels (`grid.columns` by `grid.rows`), the position in the picture's
  /// pixels the vertex reads.
  package static func warp(
    picture: UnsafePointer<UInt8>, pictureSize: (width: Int, height: Int),
    output: UnsafeMutablePointer<UInt8>, outputSize: (width: Int, height: Int),
    gridX: UnsafePointer<Double>, gridY: UnsafePointer<Double>,
    grid: (columns: Int, rows: Int, cell: Int)
  ) {
    let cell = grid.cell
    let cellSize = Double(cell)
    for cj in 0..<(grid.rows - 1) {
      for ci in 0..<(grid.columns - 1) {
        let (a, b) = (cj * grid.columns + ci, (cj + 1) * grid.columns + ci)
        for py in (cj * cell)..<min((cj + 1) * cell, outputSize.height) {
          let ty = (Double(py) + 0.5 - Double(cj * cell)) / cellSize
          let leftX = gridX[a] + (gridX[b] - gridX[a]) * ty
          let leftY = gridY[a] + (gridY[b] - gridY[a]) * ty
          let stepX = (gridX[a + 1] + (gridX[b + 1] - gridX[a + 1]) * ty - leftX) / cellSize
          let stepY = (gridY[a + 1] + (gridY[b + 1] - gridY[a + 1]) * ty - leftY) / cellSize
          // The first pixel centre is half a pixel into the cell.
          var u = Float(leftX + stepX * 0.5)
          var v = Float(leftY + stepY * 0.5)
          let (du, dv) = (Float(stepX), Float(stepY))
          for px in (ci * cell)..<min((ci + 1) * cell, outputSize.width) {
            store(
              sample(
                picture, width: pictureSize.width, height: pictureSize.height, u: u, v: v),
              output, py * outputSize.width + px)
            u += du
            v += dv
          }
        }
      }
    }
  }
}
