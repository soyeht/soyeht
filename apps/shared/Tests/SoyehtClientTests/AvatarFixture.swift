import Foundation
import ImageIO
import SoyehtAvatarRaster
import SoyehtProtocol
import UniformTypeIdentifiers

@testable import SoyehtClient

/// A synthetic avatar package of any canvas size, with a mouth mesh of a few thousand triangles,
/// for the rasteriser's tests and its frame cost: the drawings are plain gradients, so a test can
/// say what a pixel must be. It goes through `AvatarPackage.load`, so the fixture meets the rules.
enum AvatarFixture {
  static let atlasColumns = 3
  static let atlasRows = 3
  /// The lab's face, in a 200-pixel canvas, scaled to the canvas of the fixture.
  private static let referenceCanvas = 200.0

  struct Made {
    let package: AvatarPackage
    let images: [String: AvatarBitmap]
    let canvas: Int
    /// The document, as `avatar.json` holds it.
    let json: Data

    /// The files the package lists, as a caller that has read the folder would report them.
    var assets: [AvatarAssetInfo] {
      images.map { path, bitmap in
        AvatarAssetInfo(
          path: path, bytes: 100,
          size: AvatarSize(width: UInt32(bitmap.width), height: UInt32(bitmap.height)))
      }
    }
  }

  /// The colour of the eye frame of an id: each frame is a flat patch of its own.
  static func eyeColor(_ id: String) -> [UInt8] {
    let index = UInt8(AvatarPackage.eyeIds.firstIndex(of: id) ?? 0)
    return [40 + index * 25, 200 - index * 20, 90, 255]
  }

  /// A picture of flat colour or of a function of the pixel.
  static func bitmap(_ width: Int, _ height: Int, _ pixel: (Int, Int) -> [UInt8]) -> AvatarBitmap {
    var bitmap = AvatarBitmap(width: width, height: height)
    for y in 0..<height {
      for x in 0..<width {
        bitmap.pixels.replaceSubrange(
          (y * width + x) * 4..<(y * width + x) * 4 + 4, with: pixel(x, y))
      }
    }
    return bitmap
  }

  /// The base picture: stripes (so a wrong source position is seen) over a gradient.
  static func basePixel(_ x: Int, _ y: Int) -> [UInt8] {
    let stripe = 0.5 + 0.5 * sin(Double(x + 2 * y) * 0.21)
    return [UInt8(40 + 150 * stripe), UInt8(60 + x % 128), UInt8(80 + y % 128), 255]
  }

  /// How the inner mouth (teeth and tongue) is built.
  enum Inner {
    /// No height while the lips are together, and no picture of its own then: a real mesh.
    case collapsed
    /// A picture of its own for every drawing, and a height in every one.
    case textured
    /// No picture while the lips are together, but a height all the same.
    case untexturedAtRest
  }

  static func make(
    canvas: Int = 1024, columns: Int = 36, rows: Int = 24, inner: Inner = .collapsed,
    mouth size: Double = 1
  ) throws -> Made {
    let k = Double(canvas) / referenceCanvas
    let atlasCell = (width: 96, height: 64)
    let atlasSize = (width: atlasCell.width * atlasColumns, height: atlasCell.height * atlasRows)
    var images: [String: AvatarBitmap] = [
      "base.png": bitmap(canvas, canvas, basePixel),
      "atlas.png": bitmap(atlasSize.width, atlasSize.height) { x, y in
        let cell = (y / atlasCell.height) * atlasColumns + x / atlasCell.width
        return [UInt8(30 + cell * 25), UInt8(220 - cell * 15), UInt8(100 + (x % 96)), 255]
      },
    ]
    var files: [JsonValue] = [
      file("base.png", canvas, canvas), file("atlas.png", atlasSize.width, atlasSize.height),
    ]
    let patch = (width: Int(0.3 * Double(canvas)), height: Int(0.06 * Double(canvas)))
    var eyes: [JsonValue] = []
    for (i, id) in AvatarPackage.eyeIds.enumerated() {
      let color = eyeColor(id)
      images["frames/\(id).png"] = bitmap(patch.width, patch.height) { _, _ in color }
      files.append(file("frames/\(id).png", patch.width, patch.height))
      let x = Int(0.2 * Double(canvas)) + (i % 2) * 2
      eyes.append(
        .object([
          "id": .string(id), "file": .string("frames/\(id).png"),
          "rect": ints([x, Int(0.3 * Double(canvas)), patch.width, patch.height]),
        ]))
    }
    let doc: JsonValue = .object([
      "format": .string("soyeht-avatar"), "version": .string("0.1"),
      "canvas": .object(["width": .integer(Int64(canvas)), "height": .integer(Int64(canvas))]),
      "files": .array(files), "base": .string("base.png"),
      "mouth": .object([
        "atlas": .object([
          "file": .string("atlas.png"), "columns": .integer(Int64(atlasColumns)),
          "rows": .integer(Int64(atlasRows)),
        ]),
        "parts": .array(
          [("skin", 1.0), ("lips", 0.55), ("inner", 0.45)].map {
            part(
              $0.0, scale: $0.1 * size, canvas: canvas, columns: columns, rows: rows, inner: inner)
          }),
      ]),
      "eyes": .array(eyes), "warp": warp(k),
    ])
    let data = try JSONEncoder().encode(doc)
    let assets = images.map { path, bitmap in
      AvatarAssetInfo(
        path: path, bytes: 100,
        size: AvatarSize(width: UInt32(bitmap.width), height: UInt32(bitmap.height)))
    }
    return Made(
      package: try AvatarPackage.load(json: data, assets: assets), images: images, canvas: canvas,
      json: data)
  }

  private static func file(_ path: String, _ width: Int, _ height: Int) -> JsonValue {
    .object([
      "path": .string(path), "width": .integer(Int64(width)), "height": .integer(Int64(height)),
    ])
  }

  private static func ints(_ xs: [Int]) -> JsonValue { .array(xs.map { .integer(Int64($0)) }) }

  private static func doubles(_ xs: [Double]) -> JsonValue { .array(xs.map { .number($0) }) }

  private static func region(_ c: [Double], _ r: [Double], _ inner: Double, _ k: Double)
    -> JsonValue
  {
    .object([
      "center": doubles(c.map { $0 * k }), "radii": doubles(r.map { $0 * k }),
      "inner": .number(inner),
    ])
  }

  private static func warp(_ k: Double) -> JsonValue {
    .object([
      "pivot": doubles([100 * k, 127 * k]), "zoomCenter": doubles([100 * k, 109 * k]),
      "head": region([100, 75], [72, 76], 0.74, k), "headFade": doubles([117 * k, 143 * k]),
      "face": region([100, 89], [40, 42], 0.55, k),
      "brows": .array([region([84, 62], [15, 5], 0.45, k), region([115, 61], [15, 5], 0.45, k)]),
      "browTiltSpan": .number(14 * k), "jaw": region([100, 106], [29, 19], 0.5, k),
      "cheek": region([100, 92], [37, 11], 0.3, k),
      "chest": .object([
        "from": .number(125 * k), "to": .number(156 * k), "centerX": .number(100 * k),
        "spread": .number(0.006), "rise": .number(0.7 * k),
      ]),
    ])
  }

  /// How open and how wide each drawing is, as factors of the mouth box: the shapes differ, so a
  /// blend of two of them is a different mouth from either.
  private static func factors(
    _ shape: AvatarShape, id: String, inner mode: Inner
  ) -> (open: Double, width: Double) {
    let a = shape.anchor
    let width = 0.6 + 0.4 * a.w
    guard id == "inner" else { return (0.15 + 0.85 * a.o, width) }
    // A collapsed inner mouth has no height while the lips are together, as in a real mesh; any
    // other has a height of its own in every drawing.
    return (mode == .collapsed ? a.o : 0.6 + 0.4 * a.o, width)
  }

  private static func part(
    _ id: String, scale: Double, canvas: Int, columns: Int, rows: Int, inner mode: Inner
  ) -> JsonValue {
    let k = Double(canvas) / referenceCanvas
    // A mouth of about 11 by 7 hundredths of the canvas: the work of the mouth, at 0.55 canvases,
    // is under the limit of one, as the lab's (0.23) is.
    let box = (width: 22 * k * scale, height: 14 * k * scale)
    let center = (x: 100 * k, y: 100 * k)
    var grid: [(Double, Double)] = []
    for j in 0..<rows {
      for i in 0..<columns {
        grid.append((Double(i) / Double(columns - 1), Double(j) / Double(rows - 1)))
      }
    }
    var triangles: [Int] = []
    for j in 0..<(rows - 1) {
      for i in 0..<(columns - 1) {
        let a = j * columns + i
        triangles += [a, a + 1, a + columns, a + 1, a + columns + 1, a + columns]
      }
    }
    let alphas: [Double] = grid.map { (g: (Double, Double)) -> Double in
      let edge: Double = min(min(g.0, 1 - g.0), min(g.1, 1 - g.1))
      return min(1.0, 4.0 * edge)
    }
    let shapes: [JsonValue] = AvatarShape.allCases.map { shape in
      let f = factors(shape, id: id, inner: mode)
      var positions: [Double] = []
      for g in grid {
        positions.append(center.x + (g.0 - 0.5) * box.width * f.width)
        positions.append(center.y + (g.1 - 0.5) * box.height * f.open)
      }
      let cell = shape.rawValue
      let (col, row) = (cell % atlasColumns, cell / atlasColumns)
      var uvs: [Double] = []
      for g in grid {
        uvs.append((Double(col) + 0.01 + 0.98 * g.0) / Double(atlasColumns))
        uvs.append((Double(row) + 0.01 + 0.98 * g.1) / Double(atlasRows))
      }
      var object: [String: JsonValue] = ["id": .string(shape.id), "positions": doubles(positions)]
      // The inner mouth has no picture of its own while the lips are closed, unless it is textured.
      let closed = shape == .rest || shape == .closedMBP
      if !(id == "inner" && closed && mode != .textured) {
        object["uvs"] = doubles(uvs)
      }
      return .object(object)
    }
    return .object([
      "id": .string(id), "triangles": ints(triangles), "alphas": doubles(alphas),
      "texturePower": .number(id == "skin" ? 2 : 1), "hideWhenEmpty": .bool(id == "inner"),
      "shapes": .array(shapes),
    ])
  }

  /// The frame of a mouth at these weights (nine, summing to 1), with the pose and the eyes.
  static func frame(
    weights: [Double], pose: AvatarPose = AvatarPose(),
    eye: AvatarEye = AvatarEye(gaze: .center, lid: .open, soft: false)
  ) -> AvatarFrame {
    AvatarFrame(
      mouth: .rest, weights: AvatarMorphWeights(visemes: Array(weights.dropFirst())),
      acting: AvatarActingFrame(pose: pose, eye: eye, state: .idle))
  }

  /// A weight vector of one drawing, or of a blend of two.
  static func weights(_ shape: AvatarShape, _ other: AvatarShape? = nil, mix: Double = 0)
    -> [Double]
  {
    var w = [Double](repeating: 0, count: AvatarShape.allCases.count)
    w[shape.rawValue] = 1 - mix
    if let other { w[other.rawValue] += mix }
    return w
  }

  /// The PNG file of a picture.
  static func png(_ bitmap: AvatarBitmap) -> Data {
    bitmap.cgImage.flatMap(png(from:)) ?? Data()
  }

  static func png(from image: CGImage) -> Data? {
    let out = NSMutableData()
    guard
      let destination = CGImageDestinationCreateWithData(
        out, UTType.png.identifier as CFString, 1, nil)
    else { return nil }
    CGImageDestinationAddImage(destination, image, nil)
    return CGImageDestinationFinalize(destination) ? out as Data : nil
  }

  /// Write the package into a new folder: `avatar.json` and the pictures, each as a PNG.
  static func write(_ made: Made, to folder: URL) throws {
    try FileManager.default.createDirectory(
      at: folder.appendingPathComponent("frames"), withIntermediateDirectories: true)
    try made.json.write(to: folder.appendingPathComponent("avatar.json"))
    for (path, bitmap) in made.images {
      try png(bitmap).write(to: folder.appendingPathComponent(path))
    }
  }
}

/// Triangles drawn as one part by the rasteriser's two passes: mixed (one layer, alpha 1) and laid
/// over a flat picture, as a frame does.
enum AvatarTestRaster {
  typealias Triangle = (AvatarRasterizer.Vertex, AvatarRasterizer.Vertex, AvatarRasterizer.Vertex)

  /// How many pixels the triangles covered, and the picture they leave.
  static func draw(
    _ triangles: [Triangle], texture: [UInt8], textureSize: (width: Int, height: Int) = (1, 1),
    under: UInt8 = 0, opacity: Double = 1, size: (width: Int, height: Int)
  ) -> (covered: Int, picture: [UInt8]) {
    var destination = [UInt8](repeating: under, count: size.width * size.height * 4)
    let beneath = destination
    var mix = [Float](repeating: 0, count: size.width * size.height * 4)
    var covered = 0
    texture.withUnsafeBufferPointer { tex in
      beneath.withUnsafeBufferPointer { below in
        destination.withUnsafeMutableBufferPointer { dst in
          mix.withUnsafeMutableBufferPointer { mixed in
            guard let tex = tex.baseAddress, let below = below.baseAddress,
              let dst = dst.baseAddress, let mixed = mixed.baseAddress
            else { return }
            for t in triangles {
              covered += AvatarRasterizer.mixLayer(
                t.0, t.1, t.2, alpha: 1, texture: tex, textureSize: textureSize, mix: mixed,
                size: size)
            }
            for t in triangles {
              AvatarRasterizer.lay(
                t.0, t.1, t.2, opacity: opacity, mix: mixed, under: below, destination: dst,
                size: size)
            }
          }
        }
      }
    }
    return (covered, destination)
  }
}
