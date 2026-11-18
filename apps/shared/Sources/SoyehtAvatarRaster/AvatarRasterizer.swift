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

  /// The pixels a triangle covers, and for each its barycentric weights and its interpolated vertex
  /// opacity (the feather): `body(x, y, b0, b1, b2)`, with the weights of `a`, `b` and `c`, is called once for each pixel centre inside
  /// the triangle (the top-left rule decides a centre on an edge), and the count of them is
  /// returned. A triangle with no area, or with a vertex that is not a number, covers nothing.
  ///
  /// The cost is that of the pixels covered: each row is cut to the span the three edges leave,
  /// and only the pixels of the span (and one on each side, for the rounding of the cut) are
  /// tested.
  @inline(__always)
  private static func cover(
    _ a: Vertex, _ b: Vertex, _ c: Vertex, size: (width: Int, height: Int),
    _ body: (Int, Int, Double, Double, Double) -> Void
  ) -> Int {
    var (v0, v1, v2) = (a, b, c)
    var area = (v1.x - v0.x) * (v2.y - v0.y) - (v1.y - v0.y) * (v2.x - v0.x)
    guard area.isFinite, abs(area) > 1e-9 else { return 0 }
    let swapped = area < 0
    if swapped {
      swap(&v1, &v2)
      area = -area
    }
    // The pixels whose centres the triangle's box covers, bounded in doubles first: a position a
    // host got wrong must not be an integer overflow.
    let lowX = max((min(v0.x, v1.x, v2.x) - 0.5).rounded(.up), 0)
    let highX = min((max(v0.x, v1.x, v2.x) - 0.5).rounded(.down), Double(size.width - 1))
    let lowY = max((min(v0.y, v1.y, v2.y) - 0.5).rounded(.up), 0)
    let highY = min((max(v0.y, v1.y, v2.y) - 0.5).rounded(.down), Double(size.height - 1))
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
      guard !span.isEmpty else { continue }
      for x in span {
        let px = Double(x) + 0.5
        let w0 = (v2.x - v1.x) * (py - v1.y) - (v2.y - v1.y) * (px - v1.x)
        let w1 = (v0.x - v2.x) * (py - v2.y) - (v0.y - v2.y) * (px - v2.x)
        let w2 = (v1.x - v0.x) * (py - v0.y) - (v1.y - v0.y) * (px - v0.x)
        guard w0 > 0 || (w0 == 0 && own0), w1 > 0 || (w1 == 0 && own1),
          w2 > 0 || (w2 == 0 && own2)
        else { continue }
        covered += 1
        // The weights come back in the order the caller gave the vertices.
        body(x, y, w0 / area, (swapped ? w2 : w1) / area, (swapped ? w1 : w2) / area)
      }
    }
    return covered
  }

  /// One layer mixed over the mix before it: `texel * alpha + mix * (1 - alpha)`, every colour
  /// premultiplied RGBA of 0...255. The texel's own opacity is part of the colour that is mixed:
  /// a clear texel of a heavy drawing makes the mix clearer in proportion to its weight, and a
  /// drawing that comes in at a tiny weight changes the mix by a tiny amount.
  package static func mixStep(_ mix: SIMD4<Float>, texel: SIMD4<Float>, alpha: Float)
    -> SIMD4<Float>
  {
    let a = Float.minimum(Float.maximum(alpha, 0), 1)
    return texel * a + mix * (1 - a)
  }

  /// A mixed part laid over `under` by `coverage`, 0 to 1: `mix * c + under * (1 - c * mix alpha /
  /// 255)`. The feather counts once, in `coverage`, however many drawings were mixed.
  package static func compose(mix: SIMD4<Float>, under: SIMD4<Float>, coverage: Float)
    -> SIMD4<Float>
  {
    let c = Float.minimum(Float.maximum(coverage, 0), 1)
    return mix * c + under * (1 - c * mix[3] / 255)
  }

  /// Mix one layer of a part into `mix` (premultiplied RGBA of 0...255, a float for each channel):
  /// the texel the drawing's picture gives at the pixel, with the layer's `alpha`, over the mix
  /// before it: `mix = texel * alpha + mix * (1 - alpha)`. The texel's own opacity is part of the
  /// colour that is mixed. Says how many pixels the triangle covered. The clearing of `mix` before
  /// the first layer, and the laying of the mix over the picture (`lay`), are the caller's.
  @discardableResult
  package static func mixLayer(
    _ a: Vertex, _ b: Vertex, _ c: Vertex, alpha: Double, texture: UnsafePointer<UInt8>,
    textureSize: (width: Int, height: Int), mix: UnsafeMutablePointer<Float>,
    size: (width: Int, height: Int)
  ) -> Int {
    let layer = Float(alpha)
    return cover(a, b, c, size: size) { x, y, b0, b1, b2 in
      let texel = sample(
        texture, width: textureSize.width, height: textureSize.height,
        u: Float(b0 * a.u + b1 * b.u + b2 * c.u), v: Float(b0 * a.v + b1 * b.v + b2 * c.v))
      let at = (y * size.width + x) * 4
      let before = SIMD4<Float>(mix[at], mix[at + 1], mix[at + 2], mix[at + 3])
      let mixed = mixStep(before, texel: texel, alpha: layer)
      mix[at] = mixed[0]
      mix[at + 1] = mixed[1]
      mix[at + 2] = mixed[2]
      mix[at + 3] = mixed[3]
    }
  }

  /// Lay the mix of a part over `under` (the picture as it was before the part) into
  /// `destination`, once: with the coverage `c` of the pixel, the part's `opacity` times the
  /// feather of its corners interpolated there, `out = mix * c + under * (1 - c * mix alpha / 255)`.
  /// The feather counts once however many drawings are mixed. Says how many pixels the triangle
  /// covered.
  @discardableResult
  package static func lay(
    _ a: Vertex, _ b: Vertex, _ c: Vertex, opacity: Double, mix: UnsafePointer<Float>,
    under: UnsafePointer<UInt8>, destination: UnsafeMutablePointer<UInt8>,
    size: (width: Int, height: Int)
  ) -> Int {
    cover(a, b, c, size: size) { x, y, b0, b1, b2 in
      let feather = b0 * a.alpha + b1 * b.alpha + b2 * c.alpha
      let coverage = Float(opacity * feather)
      let index = y * size.width + x
      let at = index * 4
      let colour = SIMD4<Float>(mix[at], mix[at + 1], mix[at + 2], mix[at + 3])
      store(compose(mix: colour, under: load(under, index), coverage: coverage), destination, index)
    }
  }

  /// The pixels of the row `py` (a pixel centre's y) that can be inside the triangle: the
  /// interval its three edge functions leave, one pixel wider on each side for rounding, cut to
  /// `from...to`. Empty (`0..<0`) when the row misses the triangle: a range whose bounds are
  /// the wrong way round cannot be built, and a triangle may well leave the picture.
  private static func span(
    row py: Double, _ v0: Vertex, _ v1: Vertex, _ v2: Vertex, from: Int, to: Int
  ) -> Range<Int> {
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
    guard cut(v1, v2), cut(v2, v0), cut(v0, v1) else { return 0..<0 }
    // Pixel x has its centre at x + 0.5.
    let first = max((low - 0.5).rounded(.up) - 1, Double(from))
    let last = min((high - 0.5).rounded(.down) + 1, Double(to))
    guard first <= last else { return 0..<0 }
    return Int(first)..<(Int(last) + 1)
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
