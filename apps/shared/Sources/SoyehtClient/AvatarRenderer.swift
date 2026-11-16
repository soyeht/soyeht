import CoreGraphics
import Foundation

/// An axis-aligned rectangle of whole pixels, `x0..<x1` by `y0..<y1`.
struct AvatarPixelRect: Equatable, Sendable {
  var x0: Int
  var y0: Int
  var x1: Int
  var y1: Int

  var isEmpty: Bool { x0 >= x1 || y0 >= y1 }

  func union(_ other: AvatarPixelRect) -> AvatarPixelRect {
    if isEmpty { return other }
    if other.isEmpty { return self }
    return AvatarPixelRect(
      x0: min(x0, other.x0), y0: min(y0, other.y0), x1: max(x1, other.x1), y1: max(y1, other.y1))
  }
}

/// Draws one avatar's frames: the base picture with the eye frame and the continuous mouth drawn
/// into it, then the whole picture warped by the pose of the head (ADR 0166, section 6).
///
/// The mouth is geometry: the lip contours of the nine drawings are blended by the morph weights
/// into one mesh, and each textured drawing is drawn over the previous ones through its own texture
/// coordinates with the opacity that makes the layers their exact weighted average. There is no
/// code path that picks a drawing. The warp reads the picture from the points `AvatarWarp` says,
/// on a coarse grid (`cell` output pixels wide) whose positions are interpolated across each cell.
///
/// Everything is drawn at the size of the output: the pictures are scaled once when the renderer is
/// made (averaged, so a canvas of 1024 pixels drawn at 512 does not shimmer), and the frame costs
/// what the output costs, not what the canvas does. One instance draws on one thread at a time:
/// it keeps its buffers between frames, and only the pixels that changed since the last frame (the
/// eye frame and the mouth) are restored from the base picture.
final class AvatarRenderer {
  let package: AvatarPackage
  /// The size of the pictures `render` returns.
  let outputWidth: Int
  let outputHeight: Int
  /// The longer side of the output.
  var outputSide: Int { max(outputWidth, outputHeight) }

  /// An eye frame at the output's scale, with where it goes. It holds the base picture around the
  /// eye as well, so it replaces the pixels it covers.
  private struct Patch {
    let bitmap: AvatarBitmap
    let x: Int
    let y: Int
  }

  private let base: AvatarBitmap
  private var source: AvatarBitmap
  private let atlas: AvatarBitmap
  private let patches: [String: Patch]
  /// The mouth's parts in drawing order, with positions in output pixels and texture coordinates
  /// in atlas pixels, by shape.
  private let parts: [(part: AvatarMeshPart, uvs: [[Double]?])]
  private var output: AvatarBitmap
  /// The picture under the part being drawn (a copy of the rows it touches).
  private var underneath: [UInt8]
  private let cell: Int
  /// Output pixels per canvas pixel.
  private let scale: Double
  private var dirty = AvatarPixelRect(x0: 0, y0: 0, x1: 0, y1: 0)
  private var gridX: [Double] = []
  private var gridY: [Double] = []

  /// A renderer for a validated package and its pictures, by the path the package lists them
  /// under. The picture is drawn no larger than `maxSide` pixels on its longer side, and never
  /// larger than the canvas. `cell` is the width of a cell of the warp grid, in output pixels.
  init(package: AvatarPackage, images: [String: AvatarBitmap], maxSide: Int = 512, cell: Int = 8)
    throws
  {
    func image(_ path: String) throws -> AvatarBitmap {
      guard let found = images[path] else { throw AvatarPackageError(kind: .file, at: path) }
      return found
    }
    self.package = package
    let canvas = (width: Int(package.canvas.width), height: Int(package.canvas.height))
    let scale = min(1, Double(max(maxSide, 1)) / Double(max(canvas.width, canvas.height)))
    self.scale = scale
    let scaled = { (n: Int) in max(1, Int((Double(n) * scale).rounded())) }
    outputWidth = scaled(canvas.width)
    outputHeight = scaled(canvas.height)
    let full = try image(package.base)
    guard full.width == canvas.width, full.height == canvas.height else {
      throw AvatarPackageError(kind: .canvas, at: "/base")
    }
    base = full.resized(width: outputWidth, height: outputHeight)
    source = base
    output = AvatarBitmap(width: outputWidth, height: outputHeight)
    underneath = [UInt8](repeating: 0, count: outputWidth * outputHeight * 4)
    let fullAtlas = try image(package.mouth.atlas.file)
    atlas = fullAtlas.resized(width: scaled(fullAtlas.width), height: scaled(fullAtlas.height))
    // An eye frame is laid over the base picture at the size of the canvas, and then the picture is
    // scaled and the rectangle (rounded outward) is cut out: the frame lands where the scaled
    // picture puts the eye, not up to half a pixel away.
    var patches: [String: Patch] = [:]
    for frame in package.eyes {
      let picture = try image(frame.file)
      let (x, y) = (Int(frame.rect[0]), Int(frame.rect[1]))
      let laid = full.laying(picture, atX: x, y: y)
        .resized(width: outputWidth, height: outputHeight)
      let x0 = Int((Double(x) * scale).rounded(.down))
      let y0 = Int((Double(y) * scale).rounded(.down))
      let x1 = Int((Double(x + picture.width) * scale).rounded(.up))
      let y1 = Int((Double(y + picture.height) * scale).rounded(.up))
      patches[frame.id] = Patch(
        bitmap: laid.cropped(
          AvatarPixelRect(
            x0: max(x0, 0), y0: max(y0, 0), x1: min(x1, outputWidth), y1: min(y1, outputHeight))),
        x: max(x0, 0), y: max(y0, 0))
    }
    self.patches = patches
    self.cell = max(cell, 1)
    let (atlasWidth, atlasHeight) = (Double(atlas.width), Double(atlas.height))
    parts = AvatarPartKind.allCases.compactMap { kind in
      package.mouth.parts.first { $0.id == kind }
    }.map { original in
      var part = original
      for index in part.shapes.indices {
        part.shapes[index].positions = part.shapes[index].positions.map { $0 * scale }
      }
      let uvs = AvatarShape.ids.map { id -> [Double]? in
        guard let uv = part.shapes.first(where: { $0.id == id })?.uvs else { return nil }
        return stride(from: 0, to: uv.count, by: 2).flatMap {
          [uv[$0] * atlasWidth, uv[$0 + 1] * atlasHeight]
        }
      }
      return (part, uvs)
    }
  }

  /// The eye drawing a frame needs: the blink, the half-closed lid, where the eyes look, or the
  /// soft eyes of listening. `nil` is the base picture's own open eyes.
  static func patchId(for eye: AvatarEye) -> String? {
    switch eye.lid {
    case .closed: "blink"
    case .half: "half"
    case .open: eye.gaze != .center ? eye.gaze.rawValue : (eye.soft ? "soft" : nil)
    }
  }

  /// Draw a frame.
  func render(_ frame: AvatarFrame) -> AvatarBitmap {
    restoreDirty()
    var changed = drawEyes(frame.acting.eye)
    changed = changed.union(drawMouth(weights: frame.weights.all))
    dirty = changed
    warp(frame.acting.pose)
    return output
  }

  // MARK: the picture before the warp

  private func restoreDirty() {
    guard !dirty.isEmpty else { return }
    let width = base.width
    base.pixels.withUnsafeBufferPointer { from in
      source.pixels.withUnsafeMutableBufferPointer { to in
        for y in dirty.y0..<dirty.y1 {
          let start = (y * width + dirty.x0) * 4
          let count = (dirty.x1 - dirty.x0) * 4
          to.baseAddress.map { dst in
            from.baseAddress.map { (dst + start).update(from: $0 + start, count: count) }
          }
        }
      }
    }
  }

  /// Put the eye frame in place of the base picture's pixels.
  private func drawEyes(_ eye: AvatarEye) -> AvatarPixelRect {
    guard let id = Self.patchId(for: eye), let patch = patches[id] else {
      return AvatarPixelRect(x0: 0, y0: 0, x1: 0, y1: 0)
    }
    let width = source.width
    let picture = patch.bitmap
    picture.pixels.withUnsafeBufferPointer { from in
      source.pixels.withUnsafeMutableBufferPointer { to in
        guard let from = from.baseAddress, let to = to.baseAddress else { return }
        for row in 0..<picture.height {
          (to + ((patch.y + row) * width + patch.x) * 4).update(
            from: from + row * picture.width * 4, count: picture.width * 4)
        }
      }
    }
    return AvatarPixelRect(
      x0: patch.x, y0: patch.y, x1: patch.x + picture.width, y1: patch.y + picture.height)
  }

  /// What the mouth of the last frame cost: the triangles drawn and the pixel centres tested. A
  /// frame that would test more than `mouthBudget` draws no mouth at all.
  private(set) var lastMouthWork = (triangles: 0, tests: 0)

  /// The most pixel centres a frame may test for the mouth: twice the output's pixels. A mouth
  /// is a few percent of the face; a package whose mesh covers the picture over and over (it is
  /// bounded by its vertices, not by what it covers) draws the base picture instead of costing
  /// seconds a frame.
  var mouthBudget: Int { 2 * outputWidth * outputHeight }

  /// Blend the nine drawings' contours by the weights and draw the textured layers of each part.
  private func drawMouth(weights: [Double]) -> AvatarPixelRect {
    var box = AvatarPixelRect(x0: 0, y0: 0, x1: 0, y1: 0)
    let size = (width: source.width, height: source.height)
    let atlasSize = (width: atlas.width, height: atlas.height)
    let blended = parts.map { part, _ in
      (positions: part.blend(weights), layers: part.layers(weights))
    }
    var work = (triangles: 0, tests: 0.0)
    for ((part, _), item) in zip(parts, blended) {
      let drawn = Double(item.layers.count)
      let tests = Self.tests(of: part, positions: item.positions, size: size)
      work.triangles += part.triangles.count / 3 * item.layers.count
      work.tests += tests * drawn
    }
    guard work.tests <= Double(mouthBudget) else {
      lastMouthWork = (0, 0)
      return box
    }
    lastMouthWork = (work.triangles, Int(work.tests))
    atlas.pixels.withUnsafeBufferPointer { texture in
      source.pixels.withUnsafeMutableBufferPointer { destination in
        underneath.withUnsafeMutableBufferPointer { under in
          guard let texture = texture.baseAddress, let destination = destination.baseAddress,
            let under = under.baseAddress
          else { return }
          for ((part, uvs), item) in zip(parts, blended) {
            let area = Self.bounds(of: item.positions, within: size)
            box = box.union(area)
            // What is under the part, as it was before any of its layers: the layers mix over it.
            for y in area.y0..<max(area.y1, area.y0) {
              (under + y * size.width * 4).update(
                from: destination + y * size.width * 4, count: size.width * 4)
            }
            for layer in item.layers {
              guard let uv = uvs[layer.shape.rawValue] else { continue }
              Self.draw(
                part, positions: item.positions, uvs: uv, opacity: layer.alpha, under: under,
                texture: texture, textureSize: atlasSize, destination: destination,
                destinationSize: size)
            }
          }
        }
      }
    }
    return box
  }

  /// The pixel centres the triangles of a part test, once.
  private static func tests(
    of part: AvatarMeshPart, positions: [Double], size: (width: Int, height: Int)
  ) -> Double {
    let vertex = { (i: Int) in
      AvatarRasterizer.Vertex(x: positions[2 * i], y: positions[2 * i + 1], u: 0, v: 0, alpha: 1)
    }
    var total = 0.0
    for t in stride(from: 0, to: part.triangles.count, by: 3) {
      total += AvatarRasterizer.tests(
        vertex(Int(part.triangles[t])), vertex(Int(part.triangles[t + 1])),
        vertex(Int(part.triangles[t + 2])), destinationSize: size)
    }
    return total
  }

  private static func draw(
    _ part: AvatarMeshPart, positions: [Double], uvs: [Double], opacity: Double,
    under: UnsafePointer<UInt8>, texture: UnsafePointer<UInt8>,
    textureSize: (width: Int, height: Int),
    destination: UnsafeMutablePointer<UInt8>, destinationSize: (width: Int, height: Int)
  ) {
    let vertex = { (i: Int) in
      AvatarRasterizer.Vertex(
        x: positions[2 * i], y: positions[2 * i + 1], u: uvs[2 * i], v: uvs[2 * i + 1],
        alpha: part.alphas?[i] ?? 1)
    }
    for t in stride(from: 0, to: part.triangles.count, by: 3) {
      AvatarRasterizer.fill(
        vertex(Int(part.triangles[t])), vertex(Int(part.triangles[t + 1])),
        vertex(Int(part.triangles[t + 2])), opacity: opacity, under: under, texture: texture,
        textureSize: textureSize, destination: destination, destinationSize: destinationSize)
    }
  }

  /// The pixels a mesh can touch, with a margin for the sampling of its edge.
  private static func bounds(of positions: [Double], within size: (width: Int, height: Int))
    -> AvatarPixelRect
  {
    var (x0, y0, x1, y1) = (Double.infinity, Double.infinity, -Double.infinity, -Double.infinity)
    for i in stride(from: 0, to: positions.count - 1, by: 2) {
      x0 = min(x0, positions[i])
      x1 = max(x1, positions[i])
      y0 = min(y0, positions[i + 1])
      y1 = max(y1, positions[i + 1])
    }
    guard x0 <= x1, y0 <= y1 else { return AvatarPixelRect(x0: 0, y0: 0, x1: 0, y1: 0) }
    let clampX = { (v: Double) in Int(min(max(v, 0), Double(size.width))) }
    let clampY = { (v: Double) in Int(min(max(v, 0), Double(size.height))) }
    return AvatarPixelRect(
      x0: clampX((x0 - 2).rounded(.down)), y0: clampY((y0 - 2).rounded(.down)),
      x1: clampX((x1 + 2).rounded(.up)), y1: clampY((y1 + 2).rounded(.up)))
  }

  // MARK: the warp

  /// Read the picture from where the pose says, for every output pixel.
  private func warp(_ pose: AvatarPose) {
    let columns = (outputWidth + cell - 1) / cell + 1
    let rows = (outputHeight + cell - 1) / cell + 1
    gridX = [Double](repeating: 0, count: columns * rows)
    gridY = gridX
    for j in 0..<rows {
      for i in 0..<columns {
        // The pose is in canvas pixels, the picture in output pixels.
        let point = package.warp.sourcePoint(
          pose: pose, x: Double(i * cell) / scale, y: Double(j * cell) / scale)
        gridX[j * columns + i] = point.x * scale
        gridY[j * columns + i] = point.y * scale
      }
    }
    let grid = (columns: columns, rows: rows, cell: cell)
    let (picture, outSize) = ((source.width, source.height), (outputWidth, outputHeight))
    source.pixels.withUnsafeBufferPointer { picture0 in
      output.pixels.withUnsafeMutableBufferPointer { out in
        gridX.withUnsafeBufferPointer { gx in
          gridY.withUnsafeBufferPointer { gy in
            guard let picture0 = picture0.baseAddress, let out = out.baseAddress,
              let gx = gx.baseAddress, let gy = gy.baseAddress
            else { return }
            Self.readCells(
              picture: picture0, pictureSize: picture, output: out, outputSize: outSize,
              gridX: gx, gridY: gy, grid: grid)
          }
        }
      }
    }
  }

  /// The loop of the warp, on pointers: what a frame spends most of its time in.
  private static func readCells(
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
            AvatarRasterizer.store(
              AvatarRasterizer.sample(
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
