import Foundation

/// What the caller knows about a file of the package folder: its size in bytes and, when it is a
/// PNG, its dimensions (`avatarPngSize`).
struct AvatarAssetInfo: Equatable, Sendable {
  var path: String
  var bytes: UInt64
  var size: AvatarSize?
}

/// Why a package did not load. The names are stable (camelCase) and are the ones the vector file
/// uses.
enum AvatarPackageErrorKind: String, Sendable {
  /// Not JSON, or not the shape of a package.
  case malformed
  case wrongFormat
  case unsupportedVersion
  /// The canvas is empty or too large.
  case canvas
  /// A file path that is not a relative, plain `.png` path, is listed twice, or is named but not
  /// listed.
  case file
  /// A listed file is not among the assets the caller has.
  case missingAsset
  /// An asset is not the size the package says (or is not a PNG).
  case assetSize
  /// More files, bytes, vertices or triangles than a package may have.
  case tooLarge
  /// The shapes of a part are not the nine drawings, each once.
  case shapes
  /// The nine shapes of a part do not share one vertex list, or the parts are not skin, lips and
  /// inner once each.
  case topology
  /// A triangle names a vertex that does not exist.
  case index
  /// A number is out of its range.
  case value
  /// The eye frames are not the eight a package carries, or lie outside the canvas.
  case frames
  /// The warp's places are outside the canvas or not shaped like places.
  case warp
  /// An acting constant is out of its range.
  case acting
  /// The atlas has fewer cells than shapes, or its picture is not the size of its cells.
  case atlas
}

/// A failure names a kind and where in the document it was found, never the value: the document
/// is data from a person or an agent.
struct AvatarPackageError: Error, Equatable, Sendable {
  var kind: AvatarPackageErrorKind
  /// A JSON pointer to the place in the document, such as `/mouth/parts/1/shapes/3/uvs`.
  var at: String
}

/// The limits of a package, each with the reason it has the value it has. A package is a file
/// from a stranger, so every size, count and range a reader allocates or loops by is bounded.
enum AvatarPackageLimits {
  /// The document itself, before it is parsed.
  static let documentBytes = 4 * 1024 * 1024
  /// The largest side of the canvas and of any listed picture (pixels).
  static let side: UInt32 = 2048
  /// Listed pictures.
  static let files = 64
  /// One picture file, and all of them together (bytes).
  static let fileBytes: UInt64 = 4 * 1024 * 1024
  static let totalBytes: UInt64 = 8 * 1024 * 1024
  /// The pictures once decoded, in pixels.
  static let pixels: UInt64 = 8 * 1024 * 1024
  /// A path of the package folder, and one name of it (bytes).
  static let pathBytes = 128
  static let nameBytes = 64
  /// Vertices of one part of the mouth mesh, and its triangles.
  static let vertices = 3000
  static let triangles = 6000
  /// The power that sharpens the picture's weights. A drawing's share of the picture between two
  /// is `w^p / (w^p + (1 - w)^p)`, which moves `p` times as fast as the weight at the middle: over
  /// 2 the picture would move faster than the mouth's own rate limits keep continuous, and swap.
  static let texturePower = 2.0
  /// How many canvases of work (`AvatarPackageValidator.blendWork`) the triangles of a mouth may
  /// add up to.
  static let drawArea = 1.0
}

extension AvatarPackage {
  /// Parse a package document and check every load rule against the files the caller has.
  static func load(json: Data, assets: [AvatarAssetInfo]) throws -> AvatarPackage {
    // Before anything is parsed: a document from a stranger is not read at any length.
    guard json.count <= AvatarPackageLimits.documentBytes else {
      throw AvatarPackageError(kind: .tooLarge, at: "")
    }
    // Foundation reads a document more loosely than the Rust reader. What this closes: the first
    // byte that is not blank must open an object (no byte-order mark, no UTF-16, no bare value)
    // and no NUL byte is allowed. It does not close the rest (a number written `512.0` or `1e2`
    // for an integer, a repeated key, a trailing comma): the vector cases cannot say those, and a
    // strict pre-scan with raw-document cases is a follow-up.
    guard json.first(where: { ![0x20, 0x09, 0x0A, 0x0D].contains($0) }) == UInt8(ascii: "{"),
      !json.contains(0)
    else { throw AvatarPackageError(kind: .malformed, at: "") }
    let package: AvatarPackage
    do {
      package = try JSONDecoder().decode(AvatarPackage.self, from: json)
    } catch {
      throw AvatarPackageError(kind: .malformed, at: "")
    }
    try AvatarPackageValidator(package: package, assets: assets).validate()
    return package
  }
}

/// The load rules of a package, in the order the Rust validator applies them: the first rule a
/// package breaks is the one reported.
struct AvatarPackageValidator {
  let package: AvatarPackage
  let assets: [AvatarAssetInfo]

  /// Slack for a texture coordinate at the edge of its cell.
  private static let uvSlack = 1e-3

  private func fail(_ kind: AvatarPackageErrorKind, _ at: String) -> AvatarPackageError {
    AvatarPackageError(kind: kind, at: at)
  }

  func validate() throws {
    let p = package
    guard p.format == AvatarPackage.expectedFormat else { throw fail(.wrongFormat, "/format") }
    // The format is unstable (0.x): the one version this reader reads, spelled exactly.
    guard p.version == AvatarPackage.supportedVersion else {
      throw fail(.unsupportedVersion, "/version")
    }
    let side = AvatarPackageLimits.side
    guard p.canvas.width != 0, p.canvas.height != 0, p.canvas.width <= side,
      p.canvas.height <= side
    else { throw fail(.canvas, "/canvas") }
    try checkFiles()
    try checkMouth()
    try checkFrames()
    try checkWarp()
    if let at = AvatarActingBounds.firstOutOfRange(p.acting) { throw fail(.acting, "/acting\(at)") }
  }

  /// Names Windows reserves, whatever the extension and the case.
  private static func isReserved(_ name: String) -> Bool {
    let stem =
      (name.split(separator: ".", omittingEmptySubsequences: false).first.map(String.init)
      ?? name).uppercased()
    if ["CON", "PRN", "AUX", "NUL"].contains(stem) { return true }
    let scalars = Array(stem.unicodeScalars)
    return scalars.count == 4 && (stem.hasPrefix("COM") || stem.hasPrefix("LPT"))
      && ("1"..."9").contains(scalars[3])
  }

  private static func isAsciiAlphanumeric(_ c: UInt8) -> Bool {
    (0x30...0x39).contains(c) || (0x41...0x5A).contains(c) || (0x61...0x7A).contains(c)
  }

  /// A path inside the package folder that is the same on every system: lower-case `.png`, names
  /// of plain ASCII letters, digits, `.`, `_` and `-`, no empty, hidden or dot-only names, no name
  /// that ends in a dot, no reserved Windows name. That also refuses a control character, a
  /// bidirectional override and a letter that only looks like another.
  static func isPlainPngPath(_ path: String) -> Bool {
    let bytes = Array(path.utf8)
    guard bytes.count <= AvatarPackageLimits.pathBytes, bytes.count >= 4,
      Array(bytes.suffix(4)) == Array(".png".utf8)
    else { return false }
    return bytes.split(separator: UInt8(ascii: "/"), omittingEmptySubsequences: false).allSatisfy {
      name in
      guard let first = name.first, name.count <= AvatarPackageLimits.nameBytes,
        isAsciiAlphanumeric(first), name.last != UInt8(ascii: "."),
        name.allSatisfy({
          isAsciiAlphanumeric($0) || $0 == UInt8(ascii: ".") || $0 == UInt8(ascii: "_")
            || $0 == UInt8(ascii: "-")
        })
      else { return false }
      return !isReserved(String(decoding: name, as: UTF8.self))
    }
  }

  private func checkFiles() throws {
    let p = package
    guard p.files.count <= AvatarPackageLimits.files else { throw fail(.tooLarge, "/files") }
    // Two names that differ only in case are one file on some systems.
    var seen = Set<String>()
    var (total, pixels) = (UInt64(0), UInt64(0))
    for (i, file) in p.files.enumerated() {
      let at = "/files/\(i)"
      if !Self.isPlainPngPath(file.path) || !seen.insert(Self.asciiLowercased(file.path)).inserted {
        throw fail(.file, at)
      }
      let side = AvatarPackageLimits.side
      if file.width == 0 || file.height == 0 || file.width > side || file.height > side {
        throw fail(.assetSize, at)
      }
      guard let asset = assets.first(where: { Self.sameBytes($0.path, file.path) }) else {
        throw fail(.missingAsset, at)
      }
      if asset.bytes > AvatarPackageLimits.fileBytes { throw fail(.tooLarge, at) }
      if asset.size?.width != file.width || asset.size?.height != file.height {
        throw fail(.assetSize, at)
      }
      total += asset.bytes
      pixels += UInt64(file.width) * UInt64(file.height)
    }
    if total > AvatarPackageLimits.totalBytes || pixels > AvatarPackageLimits.pixels {
      throw fail(.tooLarge, "/files")
    }
    guard let base = listed(p.base) else { throw fail(.file, "/base") }
    // The base picture is the whole canvas.
    if base.width != p.canvas.width || base.height != p.canvas.height {
      throw fail(.canvas, "/base")
    }
    let named =
      [("/mouth/atlas/file", p.mouth.atlas.file)]
      + p.eyes.enumerated().map { ("/eyes/\($0.offset)/file", $0.element.file) }
    for (at, path) in named where listed(path) == nil { throw fail(.file, at) }
  }

  /// `to_ascii_lowercase`: only the ASCII letters change.
  private static func asciiLowercased(_ s: String) -> String {
    String(decoding: s.utf8.map { (0x41...0x5A).contains($0) ? $0 + 32 : $0 }, as: UTF8.self)
  }

  /// Paths are the same when their UTF-8 bytes are: Swift's `==` is canonical equivalence, which
  /// finds `K.png` (a Kelvin sign) in the listing of `K.png`, as no file system does.
  static func sameBytes(_ a: String, _ b: String) -> Bool { a.utf8.elementsEqual(b.utf8) }

  private func listed(_ path: String) -> AvatarFileDecl? {
    package.files.first { Self.sameBytes($0.path, path) }
  }

  private func checkMouth() throws {
    let atlas = package.mouth.atlas
    let cells = UInt64(atlas.columns) * UInt64(atlas.rows)
    guard atlas.columns != 0, atlas.rows != 0, cells >= UInt64(AvatarShape.allCases.count) else {
      throw fail(.atlas, "/mouth/atlas")
    }
    // The atlas picture is a whole number of cells, and the cells are the size of a mouth crop.
    guard let file = listed(atlas.file) else { throw fail(.file, "/mouth/atlas/file") }
    if file.width % atlas.columns != 0 || file.height % atlas.rows != 0 {
      throw fail(.atlas, "/mouth/atlas")
    }
    let kinds = package.mouth.parts.map(\.id)
    if kinds.count != AvatarPartKind.allCases.count
      || AvatarPartKind.allCases.contains(where: { !kinds.contains($0) })
    {
      throw fail(.topology, "/mouth/parts")
    }
    for (i, part) in package.mouth.parts.enumerated() {
      try checkMeshPart(part, at: "/mouth/parts/\(i)")
    }
    if blendWork() > AvatarPackageLimits.drawArea { throw fail(.tooLarge, "/mouth/parts") }
  }

  /// The work a mouth may ask of a renderer, in canvases: for each triangle of each part, the area
  /// of the box that holds its three corners in all nine drawings, summed over the triangles of
  /// the three parts, over the area of the canvas. A frame draws a part at a blend of the
  /// drawings, and a convex blend of corners lies inside that box, so no frame fills more than
  /// this (times the layers that take part). Run once the indices of every part are known to be in
  /// range.
  func blendWork() -> Double {
    var area = 0.0
    for part in package.mouth.parts {
      for t in stride(from: 0, to: part.triangles.count - 2, by: 3) {
        var low = [Double.infinity, Double.infinity]
        var high = [-Double.infinity, -Double.infinity]
        for shape in part.shapes {
          for corner in part.triangles[t..<t + 3] {
            for k in 0..<2 {
              let v = shape.positions[2 * Int(corner) + k]
              low[k] = avatarMin(low[k], v)
              high[k] = avatarMax(high[k], v)
            }
          }
        }
        area += (high[0] - low[0]) * (high[1] - low[1])
      }
    }
    return area / (Double(package.canvas.width) * Double(package.canvas.height))
  }

  private func checkMeshPart(_ part: AvatarMeshPart, at: String) throws {
    let order = AvatarShape.ids
    let ids = Set(part.shapes.map(\.id))
    if part.shapes.count != order.count || ids.count != order.count
      || order.contains(where: { !ids.contains($0) })
    {
      throw fail(.shapes, "\(at)/shapes")
    }
    guard let first = part.shapes.first else { throw fail(.shapes, "\(at)/shapes") }
    let floats = first.positions.count
    if floats == 0 || floats % 2 != 0 || floats / 2 > AvatarPackageLimits.vertices {
      throw fail(
        floats / 2 > AvatarPackageLimits.vertices ? .tooLarge : .topology,
        "\(at)/shapes/0/positions")
    }
    let vertices = floats / 2
    if part.triangles.isEmpty || part.triangles.count % 3 != 0 {
      throw fail(.topology, "\(at)/triangles")
    }
    if part.triangles.count / 3 > AvatarPackageLimits.triangles {
      throw fail(.tooLarge, "\(at)/triangles")
    }
    if part.triangles.contains(where: { Int($0) >= vertices }) {
      throw fail(.index, "\(at)/triangles")
    }
    if let alphas = part.alphas {
      if alphas.count != vertices { throw fail(.topology, "\(at)/alphas") }
      if alphas.contains(where: { !(0...1).contains($0) }) { throw fail(.value, "\(at)/alphas") }
    }
    if !(1...AvatarPackageLimits.texturePower).contains(part.texturePower) {
      throw fail(.value, "\(at)/texturePower")
    }
    try checkShapes(part, floats: floats, at: at)
  }

  private func checkShapes(_ part: AvatarMeshPart, floats: Int, at: String) throws {
    let (w, h) = (Double(package.canvas.width), Double(package.canvas.height))
    let atlas = package.mouth.atlas
    var textured = false
    for (i, shape) in part.shapes.enumerated() {
      let here = "\(at)/shapes/\(i)"
      if shape.positions.count != floats { throw fail(.topology, "\(here)/positions") }
      // Positions may leave the canvas a little (the feather), but never by more than its size.
      let outside = { (v: Double, side: Double) in !v.isFinite || v < -side || v > 2.0 * side }
      for k in stride(from: 0, to: floats, by: 2)
      where outside(shape.positions[k], w) || outside(shape.positions[k + 1], h) {
        throw fail(.value, "\(here)/positions")
      }
      guard let uvs = shape.uvs else {
        // A drawing with no picture of its own, in a part that is always drawn, would be replaced
        // by another drawing's picture in one frame; a part that hides when empty (the inner
        // mouth) may have some, and its opacity follows the weights of the others.
        if !part.hideWhenEmpty { throw fail(.value, "\(here)/uvs") }
        continue
      }
      textured = true
      if uvs.count != floats { throw fail(.topology, "\(here)/uvs") }
      // The shape's cell in the atlas, by its place in the order of the nine drawings.
      let cell = UInt32(AvatarShape.ids.firstIndex(of: shape.id) ?? 0)
      let (col, row) = (cell % atlas.columns, cell / atlas.columns)
      let (cw, ch) = (1.0 / Double(atlas.columns), 1.0 / Double(atlas.rows))
      let (u0, v0) = (Double(col) * cw, Double(row) * ch)
      let slack = Self.uvSlack
      for k in stride(from: 0, to: floats, by: 2) {
        let u = uvs[k]
        let v = uvs[k + 1]
        if !(u0 - slack...u0 + cw + slack).contains(u)
          || !(v0 - slack...v0 + ch + slack).contains(v)
        {
          throw fail(.value, "\(here)/uvs")
        }
      }
    }
    if !textured { throw fail(.value, "\(at)/shapes") }
  }

  private func checkFrames() throws {
    let p = package
    let found = Set(p.eyes.map(\.id))
    let ids = AvatarPackage.eyeIds
    if p.eyes.count != ids.count || found.count != ids.count
      || ids.contains(where: { !found.contains($0) })
    {
      throw fail(.frames, "/eyes")
    }
    for (i, frame) in p.eyes.enumerated() {
      let (x, y, w, h) = (frame.rect[0], frame.rect[1], frame.rect[2], frame.rect[3])
      let inside =
        w > 0 && h > 0 && UInt64(x) + UInt64(w) <= UInt64(p.canvas.width)
        && UInt64(y) + UInt64(h) <= UInt64(p.canvas.height)
      let picture = listed(frame.file)
      if !inside || picture.map({ $0.width != w || $0.height != h }) ?? true {
        throw fail(.frames, "/eyes/\(i)")
      }
    }
  }

  private static func regionOk(_ r: AvatarRegion, _ w: Double, _ h: Double) -> Bool {
    (0...w).contains(r.center[0]) && (0...h).contains(r.center[1]) && r.radii[0] > 0
      && r.radii[1] > 0 && r.radii[0] <= w && r.radii[1] <= h && r.inner >= 0 && r.inner < 1
  }

  private func checkWarp() throws {
    let p = package
    let (w, h) = (Double(p.canvas.width), Double(p.canvas.height))
    let k = p.warp
    let inside = { (at: [Double]) in (0...w).contains(at[0]) && (0...h).contains(at[1]) }
    let regions = [k.head, k.face, k.brows[0], k.brows[1], k.jaw, k.cheek]
    let fade = k.headFade
    let chest = k.chest
    // The left brow is left of the right one: the tilt of each runs from its outer to its inner
    // end. A spread of the chest over 5 % of the distance from the centre is not breathing, and
    // a rise over 2 % of the picture's height is not either.
    let ok =
      inside(k.pivot) && k.brows[0].center[0] < k.brows[1].center[0] && inside(k.zoomCenter)
      && regions.allSatisfy { Self.regionOk($0, w, h) } && 0 <= fade[0] && fade[0] < fade[1]
      && fade[1] <= h && k.browTiltSpan > 0 && k.browTiltSpan <= w && 0 <= chest.from
      && chest.from < chest.to && chest.to <= h && (0...w).contains(chest.centerX)
      && (0...0.05).contains(chest.spread) && (0...0.02 * h).contains(chest.rise)
    if !ok { throw fail(.warp, "/warp") }
  }
}
