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

  /// An eye frame at the output's scale, with where it goes.
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
    let fullAtlas = try image(package.mouth.atlas.file)
    atlas = fullAtlas.resized(width: scaled(fullAtlas.width), height: scaled(fullAtlas.height))
    var patches: [String: Patch] = [:]
    for frame in package.eyes {
      let picture = try image(frame.file)
      let (width, height) = (scaled(picture.width), scaled(picture.height))
      let x = min(Int((Double(frame.rect[0]) * scale).rounded()), outputWidth - width)
      let y = min(Int((Double(frame.rect[1]) * scale).rounded()), outputHeight - height)
      patches[frame.id] = Patch(
        bitmap: picture.resized(width: width, height: height), x: max(x, 0), y: max(y, 0))
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

  /// Lay the eye frame over the base picture (source-over, so a frame with soft edges blends).
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
          for column in 0..<picture.width {
            let s = row * picture.width + column
            let d = (patch.y + row) * width + patch.x + column
            let keep = 1 - AvatarRasterizer.load(from, s)[3] / 255
            AvatarRasterizer.store(
              AvatarRasterizer.load(from, s) + AvatarRasterizer.load(to, d) * keep, to, d)
          }
        }
      }
    }
    return AvatarPixelRect(
      x0: patch.x, y0: patch.y, x1: patch.x + picture.width, y1: patch.y + picture.height)
  }

  /// Blend the nine drawings' contours by the weights and draw the textured layers of each part.
  private func drawMouth(weights: [Double]) -> AvatarPixelRect {
    var box = AvatarPixelRect(x0: 0, y0: 0, x1: 0, y1: 0)
    let size = (width: source.width, height: source.height)
    let atlasSize = (width: atlas.width, height: atlas.height)
    atlas.pixels.withUnsafeBufferPointer { texture in
      source.pixels.withUnsafeMutableBufferPointer { destination in
        guard let texture = texture.baseAddress, let destination = destination.baseAddress else {
          return
        }
        for (part, uvs) in parts {
          let positions = part.blend(weights)
          box = box.union(Self.bounds(of: positions, within: size))
          for layer in part.layers(weights) {
            guard let uv = uvs[layer.shape.rawValue] else { continue }
            Self.draw(
              part, positions: positions, uvs: uv, opacity: layer.alpha, texture: texture,
              textureSize: atlasSize, destination: destination, destinationSize: size)
          }
        }
      }
    }
    return box
  }

  private static func draw(
    _ part: AvatarMeshPart, positions: [Double], uvs: [Double], opacity: Double,
    texture: UnsafePointer<UInt8>, textureSize: (width: Int, height: Int),
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
        vertex(Int(part.triangles[t + 2])), opacity: opacity, texture: texture,
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
    let (sourceWidth, sourceHeight) = (source.width, source.height)
    let (width, height, cell) = (outputWidth, outputHeight, cell)
    source.pixels.withUnsafeBufferPointer { picture in
      output.pixels.withUnsafeMutableBufferPointer { out in
        guard let picture = picture.baseAddress, let out = out.baseAddress else { return }
        for cj in 0..<(rows - 1) {
          for ci in 0..<(columns - 1) {
            let (a, b) = (cj * columns + ci, (cj + 1) * columns + ci)
            let cellSize = Double(cell)
            for py in (cj * cell)..<min((cj + 1) * cell, height) {
              let ty = (Double(py) + 0.5 - Double(cj * cell)) / cellSize
              let leftX = gridX[a] + (gridX[b] - gridX[a]) * ty
              let leftY = gridY[a] + (gridY[b] - gridY[a]) * ty
              let stepX = (gridX[a + 1] + (gridX[b + 1] - gridX[a + 1]) * ty - leftX) / cellSize
              let stepY = (gridY[a + 1] + (gridY[b + 1] - gridY[a + 1]) * ty - leftY) / cellSize
              let first = ci * cell
              // The first pixel centre is half a pixel into the cell.
              var u = Float(leftX + stepX * 0.5)
              var v = Float(leftY + stepY * 0.5)
              let (du, dv) = (Float(stepX), Float(stepY))
              for px in first..<min((ci + 1) * cell, width) {
                AvatarRasterizer.store(
                  AvatarRasterizer.sample(
                    picture, width: sourceWidth, height: sourceHeight, u: u, v: v),
                  out, py * width + px)
                u += du
                v += dv
              }
            }
          }
        }
      }
    }
  }
}
