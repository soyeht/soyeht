import Foundation

/// The avatar package as data: one JSON document (`avatar.json`) next to the PNG files it names.
/// Our own format (ADR 0166); the second reader of what `soyeht-avatar` reads, with the same load
/// rules (`AvatarPackageValidator`) and the same vector file (`avatar-package.json`).
///
/// The format is unstable (0.x, see `version`): a reader accepts one version exactly. Unknown
/// fields are ignored, and a field that is present must have its type (a `null` is malformed).
struct AvatarPackage: Equatable, Sendable, Decodable {
  /// The `format` every package carries.
  static let expectedFormat = "soyeht-avatar"
  /// The version this reader reads, and the only one.
  static let supportedVersion = "0.1"
  /// The seven eye frames and the blink frame, by id.
  static let eyeIds = ["half", "left", "right", "up", "down", "upleft", "soft", "blink"]

  var format: String
  var version: String
  /// The size of the avatar's picture in pixels; every rectangle and position is in it.
  var canvas: AvatarSize
  /// Every PNG of the package. A file the document names must be listed here.
  var files: [AvatarFileDecl]
  /// The base picture (the rest state: open eyes, resting mouth).
  var base: String
  var mouth: AvatarMouthModel
  /// The eight eye frames, by id.
  var eyes: [AvatarPatch]
  /// Where on this face the pose is applied.
  var warp: AvatarWarp
  var acting: AvatarActingTune

  private enum Keys: String, CodingKey {
    case format, version, canvas, files, base, mouth, eyes, warp, acting
  }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    format = try c.decode(String.self, forKey: .format)
    version = try c.decode(String.self, forKey: .version)
    canvas = try c.decode(AvatarSize.self, forKey: .canvas)
    files = try c.decode([AvatarFileDecl].self, forKey: .files)
    base = try c.decode(String.self, forKey: .base)
    mouth = try c.decode(AvatarMouthModel.self, forKey: .mouth)
    eyes = try c.decode([AvatarPatch].self, forKey: .eyes)
    warp = try c.decode(AvatarWarp.self, forKey: .warp)
    acting = c.contains(.acting) ? try c.decode(AvatarActingTune.self, forKey: .acting) : .init()
  }
}

struct AvatarSize: Equatable, Sendable, Decodable {
  var width: UInt32
  var height: UInt32
}

struct AvatarFileDecl: Equatable, Sendable, Decodable {
  /// A relative path with `/` separators, inside the package folder.
  var path: String
  var width: UInt32
  var height: UInt32
}

/// An edited region of the base picture: the PNG holds just the rectangle.
struct AvatarPatch: Equatable, Sendable, Decodable {
  var id: String
  var file: String
  /// `x, y, width, height` in the canvas.
  var rect: [UInt32]

  private enum Keys: String, CodingKey { case id, file, rect }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    id = try c.decode(String.self, forKey: .id)
    file = try c.decode(String.self, forKey: .file)
    rect = try c.decode([UInt32].self, forKey: .rect)
    guard rect.count == 4 else {
      throw DecodingError.dataCorruptedError(
        forKey: .rect, in: c, debugDescription: "Expected four numbers")
    }
  }
}

/// A soft ellipse of influence on the canvas: full strength inside `inner` (a share of the radii)
/// and falling smoothly to nothing at the radii.
struct AvatarRegion: Equatable, Sendable, Decodable {
  /// `x, y` of the centre, in canvas pixels.
  var center: [Double]
  /// Horizontal and vertical radius, in canvas pixels.
  var radii: [Double]
  /// The share of the radii (0 to under 1) inside which the influence is full.
  var inner: Double

  private enum Keys: String, CodingKey { case center, radii, inner }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    center = try c.pair(.center)
    radii = try c.pair(.radii)
    inner = try c.decode(Double.self, forKey: .inner)
  }
}

/// The chest and shoulders, which breathe: below `from` the picture rises by `rise` pixels at the
/// full breath and widens by `spread` of the distance from `centerX`, fully at `to`.
struct AvatarChest: Equatable, Sendable, Decodable {
  var from: Double
  var to: Double
  var centerX: Double
  var spread: Double
  var rise: Double
}

extension KeyedDecodingContainer {
  /// A required pair of numbers: exactly two.
  func pair(_ key: Key) throws -> [Double] {
    let values = try decode([Double].self, forKey: key)
    guard values.count == 2 else {
      throw DecodingError.dataCorruptedError(
        forKey: key, in: self, debugDescription: "Expected two numbers")
    }
    return values
  }
}

/// Where the pose of the acting layer applies on this face: the places a displacement of the
/// picture is centred on. They belong to the picture (a neck is where it is), so they are in the
/// package, in canvas pixels; `sourcePoint` is the one rule that uses them.
struct AvatarWarp: Equatable, Sendable, Decodable {
  /// The neck: the head rotates about this point.
  var pivot: [Double]
  /// The picture's zoom is centred here.
  var zoomCenter: [Double]
  /// The head (hair, face, ears), which moves and rotates as one.
  var head: AvatarRegion
  /// Rows of the picture, `from` to `to`, over which the head's influence fades out.
  var headFade: [Double]
  /// The face features (eyes, nose, mouth), which move a little more than the head's outline.
  var face: AvatarRegion
  /// The left and right brow, each lifted as a soft blob.
  var brows: [AvatarRegion]
  /// The distance along the brow over which its inner end lifts more than the outer one.
  var browTiltSpan: Double
  /// The lower face, which drops with the jaw.
  var jaw: AvatarRegion
  /// The cheeks, which lift with a smile.
  var cheek: AvatarRegion
  var chest: AvatarChest

  private enum Keys: String, CodingKey {
    case pivot, zoomCenter, head, headFade, face, brows, browTiltSpan, jaw, cheek, chest
  }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    pivot = try c.pair(.pivot)
    zoomCenter = try c.pair(.zoomCenter)
    head = try c.decode(AvatarRegion.self, forKey: .head)
    headFade = try c.pair(.headFade)
    face = try c.decode(AvatarRegion.self, forKey: .face)
    brows = try c.decode([AvatarRegion].self, forKey: .brows)
    guard brows.count == 2 else {
      throw DecodingError.dataCorruptedError(
        forKey: .brows, in: c, debugDescription: "Expected two brows")
    }
    browTiltSpan = try c.decode(Double.self, forKey: .browTiltSpan)
    jaw = try c.decode(AvatarRegion.self, forKey: .jaw)
    cheek = try c.decode(AvatarRegion.self, forKey: .cheek)
    chest = try c.decode(AvatarChest.self, forKey: .chest)
  }
}

struct AvatarMouthModel: Equatable, Sendable, Decodable {
  /// The picture of the nine drawings' mouth crops, `columns` by `rows` cells, one per shape in
  /// the order of `AvatarShape`.
  var atlas: AvatarAtlas
  /// `skin`, `lips` and `inner`, once each.
  var parts: [AvatarMeshPart]
}

extension AvatarMouthModel {
  /// The parts in the order they are drawn, each over the one before: the skin around the lips,
  /// the ring of the lips, and last the inner mouth. A package lists its parts in any order
  /// (`drawOrder` of `avatar-package.json`); a part it lacks is skipped.
  var inDrawingOrder: [AvatarMeshPart] {
    AvatarPartKind.allCases.compactMap { kind in parts.first { $0.id == kind } }
  }
}

struct AvatarAtlas: Equatable, Sendable, Decodable {
  var file: String
  var columns: UInt32
  var rows: UInt32
}

enum AvatarPartKind: String, CaseIterable, Sendable, Decodable {
  /// The skin around the lips.
  case skin
  /// The ring of the lips.
  case lips
  /// Teeth and tongue, inside the inner contour; drawn only while a drawing with an aperture
  /// takes part.
  case inner
}

/// One part of the mouth mesh: nine shapes with the same vertex list and one triangulation.
struct AvatarMeshPart: Equatable, Sendable, Decodable {
  var id: AvatarPartKind
  /// Vertex indices, three per triangle.
  var triangles: [UInt32]
  /// The opacity of each vertex (the feather at the edge of the skin); all 1 when absent.
  var alphas: [Double]?
  /// Above 1 sharpens the weights of the pictures (the nearest drawings dominate), never the
  /// geometry's.
  var texturePower: Double
  var hideWhenEmpty: Bool
  /// One entry per drawing, in any order, each id once.
  var shapes: [AvatarShapeMesh]

  private enum Keys: String, CodingKey {
    case id, triangles, alphas, texturePower, hideWhenEmpty, shapes
  }

  init(from decoder: any Decoder) throws {
    let c = try decoder.container(keyedBy: Keys.self)
    id = try c.decode(AvatarPartKind.self, forKey: .id)
    triangles = try c.decode([UInt32].self, forKey: .triangles)
    alphas = try c.decodeIfPresent([Double].self, forKey: .alphas)
    texturePower = c.contains(.texturePower) ? try c.decode(Double.self, forKey: .texturePower) : 1
    hideWhenEmpty =
      c.contains(.hideWhenEmpty) ? try c.decode(Bool.self, forKey: .hideWhenEmpty) : false
    shapes = try c.decode([AvatarShapeMesh].self, forKey: .shapes)
  }
}

struct AvatarShapeMesh: Equatable, Sendable, Decodable {
  /// `rest, closed_MBP, A, E, I, O, U, FV, LDT`.
  var id: String
  /// `x, y` pairs on screen, in the canvas's pixels.
  var positions: [Double]
  /// `u, v` pairs in the atlas (0...1) inside this shape's cell. Absent for a drawing with no
  /// picture of its own for this part (an inner mouth with the lips closed).
  var uvs: [Double]?
}

/// The size of a PNG without decoding it: the package validator compares it with what the package
/// declares.
func avatarPngSize(_ bytes: Data) -> AvatarSize? {
  let signature: [UInt8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
  let header = [UInt8](bytes.prefix(24))
  guard header.count == 24, Array(header[0..<8]) == signature,
    Array(header[12..<16]) == Array("IHDR".utf8)
  else { return nil }
  let word = { (at: Int) in
    header[at..<at + 4].reduce(UInt32(0)) { $0 << 8 | UInt32($1) }
  }
  return AvatarSize(width: word(16), height: word(20))
}
