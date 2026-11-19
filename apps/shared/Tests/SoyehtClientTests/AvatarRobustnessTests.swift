import Foundation
import SoyehtAvatarRaster
import Testing

@testable import SoyehtAvatar
@testable import SoyehtClient

/// The host's numbers are whatever a machine computes, and a package is a file from a stranger:
/// the runtime must keep every number it returns a number, and the loader must bound what it reads.

private func finite(_ frame: AvatarFrame) -> Bool {
  frame.mouth.array.allSatisfy(\.isFinite) && frame.weights.all.allSatisfy(\.isFinite)
    && frame.acting.pose.values.allSatisfy(\.isFinite)
}

@Test func theRuntimeKeepsEveryNumberFiniteWhateverTheHostHandsOver() {
  var runtime = AvatarRuntime(
    tune: AvatarTune(), acting: AvatarActingTune(), seed: 1, scale: .infinity)
  let phonemes = [
    AvatarPhoneme(p: "a", t0: 0, t1: .infinity), AvatarPhoneme(p: "m", t0: .nan, t1: 1),
  ]
  runtime.setClip(
    AvatarClip(
      phonemes: phonemes, envelope: AvatarEnvelope(db: [.nan, -.infinity], hop: .nan),
      tune: AvatarTune()
    )
    .withWords(text: "?", words: [AvatarWordStart(t: .nan, word: "x", prevEndsClause: false)]))
  let hostile: [(Double, AvatarAudio?)] = [
    (.nan, nil), (.infinity, nil), (-1, nil), (1e300, nil),
    (1.0 / 60, AvatarAudio(position: .nan, db: .nan)),
    (1.0 / 60, AvatarAudio(position: .infinity, db: -.infinity)),
    (1.0 / 60, AvatarAudio(position: -5, db: 1e300)),
    (1.0 / 60, AvatarAudio(position: 0.5, db: nil)),
  ]
  for state in [AvatarState.idle, .listening, .thinking, .speaking] {
    for (dt, audio) in hostile {
      #expect(finite(runtime.step(dt: dt, state: state, audio: audio)))
    }
  }
}

@Test func anAudioPositionThatIsNotANumberDoesNotMoveTheWordCounter() {
  var runtime = AvatarRuntime(
    tune: AvatarTune(), acting: AvatarActingTune(), seed: 2, scale: 1)
  let words = [AvatarWordStart(t: 0.1, word: "hello", prevEndsClause: false)]
  runtime.setClip(
    AvatarClip(phonemes: [], envelope: nil, tune: AvatarTune()).withWords(
      text: "hello", words: words))
  for position in [Double.nan, -1, 0.05, 0.2, .infinity, 0.3] {
    #expect(
      finite(
        runtime.step(
          dt: 1.0 / 60, state: .speaking, audio: AvatarAudio(position: position, db: -20))))
  }
}

@Test func anEnvelopeThatIsNotUsableAlignsNothing() {
  for envelope in [
    AvatarEnvelope(db: [], hop: 0.02), AvatarEnvelope(db: [-20, -20], hop: .nan),
    AvatarEnvelope(db: [-20, -20], hop: 0), AvatarEnvelope(db: [.nan, .nan], hop: 0.02),
    AvatarEnvelope(db: [-.infinity], hop: 0.02),
  ] {
    let alignment = AvatarAligner.align(
      text: "Olá, mundo", language: .portugueseBrazil, envelope: envelope)
    #expect(alignment.phonemes.isEmpty)
  }
}

@Test func aHugeTextOverShortAudioFitsNothingAndTakesBoundedTime() {
  let envelope = AvatarEnvelope(db: Array(repeating: -20, count: 50), hop: 0.02)
  let text = String(repeating: "mamãe bebe pão ", count: 20_000)
  let alignment = AvatarAligner.align(text: text, language: .portugueseBrazil, envelope: envelope)
  #expect(alignment.fits == false && alignment.phonemes.isEmpty)
}

private func numbers(in value: Any) -> Int {
  switch value {
  case is Double, is UInt: 1
  default: Mirror(reflecting: value).children.reduce(0) { $0 + numbers(in: $1.value) }
  }
}

@Test func everyNumberOfTheActingTuneHasABoundAndTheDefaultsAreInside() {
  #expect(AvatarActingBounds.firstOutOfRange(AvatarActingTune()) == nil)
  // Every number of the tune has a bound, so a field added later cannot escape the table.
  #expect(AvatarActingBounds.table.count == numbers(in: AvatarActingTune()))
  #expect(Set(AvatarActingBounds.table.map(\.pointer)).count == AvatarActingBounds.table.count)
  var tune = AvatarActingTune()
  tune.head.nodSpeak = 1e308
  #expect(AvatarActingBounds.firstOutOfRange(tune) == "/head/nodSpeak")
  tune = AvatarActingTune()
  tune.blink.gap = [5, 2]
  #expect(AvatarActingBounds.firstOutOfRange(tune) == "/blink/gap")
  tune = AvatarActingTune()
  tune.expression.thinking.c = .nan
  #expect(AvatarActingBounds.firstOutOfRange(tune) == "/expression/thinking/c")
}

@Test func theRasteriserDrawsNothingForATriangleThatIsNotOne() {
  let texture: [UInt8] = Array(repeating: [200, 200, 200, 255], count: 4).flatMap { $0 }
  let nan = AvatarRasterizer.Vertex(x: .nan, y: 0, u: 0, v: 0, alpha: 1)
  let a = AvatarRasterizer.Vertex(x: 0, y: 0, u: 0, v: 0, alpha: 1)
  let b = AvatarRasterizer.Vertex(x: 1e6, y: 0, u: 0, v: 0, alpha: 1)
  let c = AvatarRasterizer.Vertex(x: 0, y: 1e6, u: 0, v: 0, alpha: 1)
  let (far, farther) = (
    AvatarRasterizer.Vertex(x: 1e300, y: 0, u: 0, v: 0, alpha: 1),
    AvatarRasterizer.Vertex(x: 0, y: 1e300, u: 0, v: 0, alpha: 1)
  )
  let drawn = AvatarTestRaster.draw(
    [(nan, a, b), (a, a, a), (a, far, farther), (a, b, c)], texture: texture,
    textureSize: (2, 2), under: 7, size: (4, 4))
  // The large triangle covers the 4 by 4 picture; the others draw nothing and nothing traps.
  #expect(drawn.picture.enumerated().allSatisfy { $0.element == ($0.offset % 4 == 3 ? 255 : 200) })
}

@Test func twoTrianglesThatShareAnEdgeDrawEveryPixelOnce() {
  // Half-opaque triangles over a black picture: a pixel drawn twice would be brighter.
  let corner = { (x: Double, y: Double) in
    AvatarRasterizer.Vertex(x: x, y: y, u: 0, v: 0, alpha: 0.5)
  }
  let drawn = AvatarTestRaster.draw(
    [
      (corner(0, 0), corner(8, 0), corner(8, 8)), (corner(0, 0), corner(8, 8), corner(0, 8)),
    ], texture: [200, 200, 200, 255], size: (8, 8))
  #expect(drawn.covered == 64)
  for pixel in stride(from: 0, to: drawn.picture.count, by: 4) {
    #expect(Array(drawn.picture[pixel..<pixel + 3]) == [100, 100, 100])
    #expect(drawn.picture[pixel + 3] == 128)
  }
}

@Test func aTexelWithNoAlphaLeavesWhatIsUnderIt() {
  // The texture's own alpha is what the picture under it gives up, not only the coverage: a
  // transparent texel must not erase the face under the mouth.
  let triangle = (
    AvatarRasterizer.Vertex(x: -4, y: -4, u: 0, v: 0, alpha: 1),
    AvatarRasterizer.Vertex(x: 12, y: -4, u: 0, v: 0, alpha: 1),
    AvatarRasterizer.Vertex(x: -4, y: 12, u: 0, v: 0, alpha: 1)
  )
  func draw(over under: UInt8, texel: [UInt8]) -> [UInt8] {
    let drawn = AvatarTestRaster.draw([triangle], texture: texel, under: under, size: (4, 4))
    return Array(drawn.picture[0..<4])
  }
  #expect(draw(over: 200, texel: [0, 0, 0, 0]) == [200, 200, 200, 200])
  // Half-opaque premultiplied grey 50 over 200: 50 + 200 * (1 - 128 / 255) = 149.6, and the
  // alpha 128 + 200 * (1 - 128 / 255) = 227.6.
  #expect(draw(over: 200, texel: [50, 50, 50, 128]) == [150, 150, 150, 228])
}

@Test func aPositionThatIsNotANumberReadsAPixelAndNeverTraps() {
  let texture: [UInt8] = [10, 20, 30, 255, 40, 50, 60, 255]
  texture.withUnsafeBufferPointer { tex in
    guard let tex = tex.baseAddress else { return }
    for (u, v) in [(Float.nan, Float.nan), (.infinity, -.infinity), (1e30, 1e30)] {
      let color = AvatarRasterizer.sample(tex, width: 2, height: 1, u: u, v: v)
      #expect(color[3] == 255)
    }
  }
}

@Test func theNumbersTheRustReaderIgnoresAreIgnoredHere() {
  #expect(avatarMax(.nan, 1) == 1 && avatarMax(1, .nan) == 1 && avatarMax(2, 1) == 2)
  #expect(avatarMin(.nan, 1) == 1 && avatarMin(1, .nan) == 1 && avatarMin(2, 1) == 1)
  #expect(avatarMax(.nan, .nan).isNaN)
  // Not a number where a mouth parameter is: the weights are still weights.
  let weights = AvatarMorphWeights(params: AvatarParams(o: .nan, w: 0.5, r: 0, c: 0, f: 0)).all
  #expect(weights.allSatisfy { $0.isFinite && $0 >= 0 })
  var follow = AvatarFollow(value: 0, smooth: 0.1)
  follow.stepToward(1, smooth: .nan, dt: 1.0 / 60)
  #expect(follow.value.isFinite)
}

@Test func aDocumentThatIsNotPlainJsonObjectBytesIsMalformed() throws {
  let made = try AvatarFixture.make(canvas: 64)
  let assets = made.assets
  func kind(_ data: Data) -> AvatarPackageErrorKind? {
    do {
      _ = try AvatarPackage.load(json: data, assets: assets)
      return nil
    } catch let error as AvatarPackageError {
      return error.kind
    } catch {
      return .malformed
    }
  }
  #expect(kind(made.json) == nil)
  #expect(kind(Data("\n \t".utf8) + made.json) == nil)
  #expect(kind(Data([0xEF, 0xBB, 0xBF]) + made.json) == .malformed)
  let utf16 = String(decoding: made.json, as: UTF8.self).data(using: .utf16LittleEndian) ?? Data()
  #expect(kind(utf16) == .malformed)
  #expect(kind(Data([0x7B, 0x00, 0x7D])) == .malformed)
  #expect(kind(Data("[1]".utf8)) == .malformed)
  #expect(kind(Data()) == .malformed)
}

@Test func aPathIsTheSamePathOnlyWhenItsBytesAre() throws {
  // A Kelvin sign is canonically equivalent to a capital K, and no file system finds one in the
  // other: the base picture named with one is not the file listed with the other.
  let made = try AvatarFixture.make(canvas: 64)
  let text = String(decoding: made.json, as: UTF8.self)
    .replacingOccurrences(of: "base.png", with: "K.png")
    .replacingOccurrences(of: "\"base\":\"K.png\"", with: "\"base\":\"\\u212A.png\"")
  var assets = made.images.map {
    AvatarAssetInfo(
      path: $0.key == "base.png" ? "K.png" : $0.key, bytes: 100,
      size: AvatarSize(width: UInt32($0.value.width), height: UInt32($0.value.height)))
  }
  #expect(throws: AvatarPackageError(kind: .file, at: "/base")) {
    _ = try AvatarPackage.load(json: Data(text.utf8), assets: assets)
  }
  assets.removeAll()
  #expect(AvatarPackageValidator.sameBytes("K", "\u{212A}") == false)
  #expect("K" == "\u{212A}")
}

/// A triangle drawn over a `size` by `size` picture of one flat texel, and how many pixels it
/// covered.
private func covered(
  _ a: (Double, Double), _ b: (Double, Double), _ c: (Double, Double), size: Int = 64
) -> (count: Int, picture: [UInt8]) {
  let vertex = { (p: (Double, Double)) in
    AvatarRasterizer.Vertex(x: p.0, y: p.1, u: 0, v: 0, alpha: 1)
  }
  let drawn = AvatarTestRaster.draw(
    [(vertex(a), vertex(b), vertex(c))], texture: [10, 20, 30, 255], size: (size, size))
  return (drawn.covered, drawn.picture)
}

/// The pixels a triangle covers by the plain rule, tested at every pixel of the picture: the three
/// edge functions positive, and zero only on an edge the triangle owns.
private func brute(
  _ a: (Double, Double), _ b: (Double, Double), _ c: (Double, Double), size: Int = 64
) -> Int {
  var (p0, p1, p2) = (a, b, c)
  let area = (p1.0 - p0.0) * (p2.1 - p0.1) - (p1.1 - p0.1) * (p2.0 - p0.0)
  if area < 0 { swap(&p1, &p2) }
  let owns = { (dx: Double, dy: Double) in dy < 0 || (dy == 0 && dx > 0) }
  let edges = [(p1, p2), (p2, p0), (p0, p1)]
  var count = 0
  for y in 0..<size {
    for x in 0..<size {
      let (px, py) = (Double(x) + 0.5, Double(y) + 0.5)
      let inside = edges.allSatisfy { p, q in
        let w = (q.0 - p.0) * (py - p.1) - (q.1 - p.1) * (px - p.0)
        return w > 0 || (w == 0 && owns(q.0 - p.0, q.1 - p.1))
      }
      if inside { count += 1 }
    }
  }
  return count
}

@Test func aTriangleThatLeavesThePictureIsClippedAtEveryBorderAndNeverTraps() {
  let n = 64.0
  let cases: [(String, (Double, Double), (Double, Double), (Double, Double))] = [
    ("across the right border", (n - 12, 0), (n + 190, 0), (n + 190, 100)),
    ("across the left border", (12, 0), (-190, 0), (-190, 100)),
    ("across the top", (0, 12), (30, -190), (60, -190)),
    ("across the bottom", (0, n - 12), (30, n + 190), (60, n + 190)),
    ("wholly right", (n + 5, 0), (n + 200, 0), (n + 200, 100)),
    ("wholly left", (-5, 0), (-200, 0), (-200, 100)),
    ("wholly above", (0, -5), (30, -200), (60, -200)),
    ("wholly below", (0, n + 5), (30, n + 200), (60, n + 200)),
    ("far right, rows inside", (500, 0), (700, 0), (700, 100)),
    ("big, around the picture", (-500, -500), (900, -500), (-500, 900)),
    ("on a corner", (n - 2, n - 2), (n + 40, n - 2), (n - 2, n + 40)),
  ]
  for (name, a, b, c) in cases {
    let drawn = covered(a, b, c)
    #expect(drawn.count == brute(a, b, c), "\(name)")
    let touched = stride(from: 0, to: drawn.picture.count, by: 4).filter { drawn.picture[$0] != 0 }
    #expect(touched.count == drawn.count, "\(name)")
  }
  #expect(covered((n + 5, 0), (n + 200, 0), (n + 200, 100)).count == 0)
  #expect(covered((-500, -500), (900, -500), (-500, 900)).count == 64 * 64)
}

@Test func triangleCoverageMatchesThePlainRuleOnManyTrianglesAroundThePicture() {
  var rng = AvatarRng(seed: 11)
  for _ in 0..<300 {
    let point = { (rng: inout AvatarRng) in (rng.range(-40, 104), rng.range(-40, 104)) }
    let (a, b, c) = (point(&rng), point(&rng), point(&rng))
    #expect(covered(a, b, c).count == brute(a, b, c), "\(a) \(b) \(c)")
  }
}
