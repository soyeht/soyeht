import Foundation
import SoyehtAvatarRaster
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private let canvas = 256

private func made() throws -> AvatarFixture.Made { try AvatarFixture.make(canvas: canvas) }

private func renderer(_ made: AvatarFixture.Made, cell: Int = 8, maxSide: Int = 128) throws
  -> AvatarRenderer
{
  try AvatarRenderer(package: made.package, images: made.images, maxSide: maxSide, cell: cell)
}

private func meanDifference(_ a: AvatarBitmap, _ b: AvatarBitmap) -> Double {
  var sum = 0
  for (x, y) in zip(a.pixels, b.pixels) { sum += abs(Int(x) - Int(y)) }
  return Double(sum) / Double(a.pixels.count)
}

private func pixel(_ bitmap: AvatarBitmap, _ x: Int, _ y: Int) -> [Int] {
  (0..<4).map { Int(bitmap.pixels[(y * bitmap.width + x) * 4 + $0]) }
}

private let restFrame = AvatarFixture.frame(weights: AvatarFixture.weights(.rest))

@Test func theFixtureMeetsTheLoadRulesAtAnySize() throws {
  for size in [64, 256, 1024] { _ = try AvatarFixture.make(canvas: size) }
}

@Test func aFaceAtRestOutsideTheMouthAndTheEyesIsTheBasePicture() throws {
  let made = try made()
  let r = try renderer(made, maxSide: 128)
  let out = r.render(restFrame)
  #expect(out.width == 128 && out.height == 128)
  // A pixel of the picture reads the base at its own centre: the corner is far from the face.
  for (x, y) in [(3, 3), (120, 6), (5, 120), (124, 124)] {
    let u = (Double(x) + 0.5) * 2
    let v = (Double(y) + 0.5) * 2
    let expected = AvatarFixture.basePixel(Int(u), Int(v))
    let got = pixel(out, x, y)
    for c in 0..<3 {
      // A source position off by a pixel changes a stripe by far more than this.
      #expect(abs(got[c] - Int(expected[c])) <= 60, "pixel \(x),\(y) channel \(c)")
    }
    #expect(got[3] == 255)
  }
}

@Test func theEyeFrameIsDrawnWhereThePackageSaysAndTheBaseComesBack() throws {
  let made = try made()
  let r = try renderer(made, maxSide: canvas)
  let rect = try #require(made.package.eyes.first { $0.id == "blink" }).rect
  let (cx, cy) = (Int(rect[0]) + Int(rect[2]) / 2, Int(rect[1]) + Int(rect[3]) / 2)
  let open = r.render(restFrame)
  let blink = r.render(
    AvatarFixture.frame(
      weights: AvatarFixture.weights(.rest),
      eye: AvatarEye(gaze: .center, lid: .closed, soft: false)))
  let color = AvatarFixture.eyeColor("blink").map { Int($0) }
  #expect(pixel(blink, cx, cy) == color)
  #expect(pixel(open, cx, cy) != color)
  // The next frame with open eyes is the first one again, byte for byte.
  let again = r.render(restFrame)
  #expect(again.pixels == open.pixels)
}

@Test func theEyeDrawingFollowsTheLidFirstThenTheGazeThenTheSoftEyes() {
  let eye = { (gaze: AvatarGaze, lid: AvatarLid, soft: Bool) in
    AvatarRenderer.patchId(for: AvatarEye(gaze: gaze, lid: lid, soft: soft))
  }
  #expect(eye(.left, .closed, true) == "blink")
  #expect(eye(.left, .half, true) == "half")
  #expect(eye(.upleft, .open, true) == "upleft")
  #expect(eye(.center, .open, true) == "soft")
  #expect(eye(.center, .open, false) == nil)
  for id in [AvatarGaze.left, .right, .up, .down, .upleft] {
    #expect(
      AvatarPackage.eyeIds.contains(
        AvatarRenderer.patchId(for: AvatarEye(gaze: id, lid: .open, soft: false)) ?? ""))
  }
}

@Test func theMouthLeavesNoTraceWhenItGoesBackToRest() throws {
  let r = try renderer(try made(), maxSide: canvas)
  let first = r.render(restFrame)
  let open = r.render(AvatarFixture.frame(weights: AvatarFixture.weights(.a)))
  #expect(meanDifference(first, open) > 0)
  #expect(r.render(restFrame).pixels == first.pixels)
}

/// The largest change of one pixel between two pictures.
private func largestChange(_ a: AvatarBitmap, _ b: AvatarBitmap) -> Int {
  zip(a.pixels, b.pixels).reduce(0) { max($0, abs(Int($1.0) - Int($1.1))) }
}

/// For every pair of drawings, the mouth morphs from one to the other in steps: the most one pixel
/// changes in a step, against the most it changes between the ends divided by the steps.
private func worstStepOverEvenStep(inner: AvatarFixture.Inner, onlyFrom: [AvatarShape]? = nil)
  throws -> (
    ratio: Double, pair: String
  )
{
  let made = try AvatarFixture.make(canvas: 128, columns: 10, rows: 7, inner: inner, mouth: 1.6)
  let r = try AvatarRenderer(package: made.package, images: made.images, maxSide: 128)
  let steps = 8
  var worst = (ratio: 0.0, pair: "")
  for a in AvatarShape.allCases {
    for b in AvatarShape.allCases where b.rawValue > a.rawValue {
      if let onlyFrom, !onlyFrom.contains(a) && !onlyFrom.contains(b) { continue }
      let frames = (0...steps).map { i in
        r.render(
          AvatarFixture.frame(weights: AvatarFixture.weights(a, b, mix: Double(i) / Double(steps))))
      }
      let ends = Double(largestChange(frames[0], frames[steps]))
      let step = Double(zip(frames, frames.dropFirst()).map { largestChange($0, $1) }.max() ?? 0)
      guard ends > 0 else { continue }
      let ratio = step / (ends / Double(steps))
      if ratio > worst.ratio { worst = (ratio, "\(a.id) to \(b.id)") }
    }
  }
  return worst
}

@Test func theMouthMorphsInSmallStepsBetweenEveryPairOfDrawings() throws {
  // The picture a mouth makes changes by a small, even amount, pixel by pixel, as the weights go
  // from one drawing to another: no step is a jump between pictures. A whole jump in one of the
  // eight steps would be a ratio of eight; six is the slack for an edge moving across a pixel
  // (the worst pair is 5.3: the mouth opening, a few pixels in a step on a small picture). Every
  // pair with a mesh built as the lab's is (an inner mouth with no height where it has no
  // picture), and the pairs with the two shut drawings with a textured inner mouth.
  let collapsed = try worstStepOverEvenStep(inner: .collapsed)
  #expect(collapsed.ratio < 6, "\(collapsed.pair): \(collapsed.ratio)")
  let textured = try worstStepOverEvenStep(inner: .textured, onlyFrom: [.rest, .closedMBP])
  #expect(textured.ratio < 6, "\(textured.pair): \(textured.ratio)")
}

/// How much the picture changes when a drawing takes a weight just over the threshold of the
/// layers (0.0001), from rest.
private func changeAtTheThreshold(inner: AvatarFixture.Inner) throws -> Int {
  let made = try AvatarFixture.make(canvas: 96, columns: 18, rows: 12, inner: inner)
  let r = try AvatarRenderer(package: made.package, images: made.images, maxSide: 96)
  let rest = r.render(AvatarFixture.frame(weights: AvatarFixture.weights(.rest)))
  let barely = r.render(
    AvatarFixture.frame(weights: AvatarFixture.weights(.rest, .a, mix: 2e-4)))
  return largestChange(rest, barely)
}

@Test func aDrawingThatBarelyTakesPartBarelyChangesThePictureWhateverTheInnerMouthHas() throws {
  // The inner mouth comes in with the weights of the drawings that have a picture of it
  // (`AvatarMeshPart.opacity`), so one that has a height where it has no picture does not appear
  // whole when a drawing passes the threshold of the layers.
  for inner in [AvatarFixture.Inner.textured, .collapsed, .untexturedAtRest] {
    let change = try changeAtTheThreshold(inner: inner)
    #expect(change < 8, "\(inner): \(change)")
  }
}

@Test func theWarpGridDrawsWhatTheWarpOfEveryPixelWould() throws {
  let made = try made()
  let pose = AvatarPose(
    rot: 3, headX: 2, headY: -3, faceX: 1, faceY: 1.5, browL: 4, browR: 2.5, browTilt: 1.5,
    jaw: 3, cheek: 1.2, breath: 1, zoom: 1.02)
  let frame = AvatarFixture.frame(weights: AvatarFixture.weights(.rest), pose: pose)
  let coarse = try renderer(made, cell: 8).render(frame)
  let exact = try renderer(made, cell: 1).render(frame)
  #expect(meanDifference(coarse, exact) < 1.0)
  let still = try renderer(made, cell: 1).render(restFrame)
  // The pose moves the picture: a warp that did nothing would fail this.
  #expect(meanDifference(exact, still) > 1.0)
}

@Test func aPictureThatTheFolderLacksIsAFileError() throws {
  let made = try made()
  var images = made.images
  images["atlas.png"] = nil
  #expect(throws: AvatarPackageError.self) {
    _ = try AvatarRenderer(package: made.package, images: images)
  }
}

@Test func anEyeFrameLandsWhereTheScaledPictureHasTheEyeAtAnyScale() throws {
  let made = try made()
  let rect = try #require(made.package.eyes.first { $0.id == "blink" }).rect
  let color = AvatarFixture.eyeColor("blink").map { Int($0) }
  for side in [100, 77, 200] {
    let r = try renderer(made, maxSide: side)
    let scale = Double(side) / Double(canvas)
    let frame = AvatarFixture.frame(
      weights: AvatarFixture.weights(.rest),
      eye: AvatarEye(gaze: .center, lid: .closed, soft: false))
    let blink = r.render(frame)
    let (x0, y0) = (Double(rect[0]) * scale, Double(rect[1]) * scale)
    let (x1, y1) = (x0 + Double(rect[2]) * scale, y0 + Double(rect[3]) * scale)
    // Every pixel inside the frame's rectangle, but the ring the scaling filter reaches across
    // its edge, has the frame's colour, and a pixel a whole pixel outside has not.
    for y in (Int(y0.rounded(.up)) + 2)..<(Int(y1.rounded(.down)) - 1) {
      for x in (Int(x0.rounded(.up)) + 2)..<(Int(x1.rounded(.down)) - 1) {
        let got = pixel(blink, x, y)
        for c in 0..<3 { #expect(abs(got[c] - color[c]) <= 3, "scale \(side): \(x),\(y)") }
      }
    }
    let outside = pixel(blink, Int(x0) - 2, Int(y0) - 2)
    #expect(outside != color)
    let after = pixel(blink, Int(x1) + 1, Int(y1) + 1)
    #expect(after != color)
  }
}

@Test func theMouthDrawsAsManyTrianglesAsItsLayersAndCostsWhatItCovers() throws {
  let made = try AvatarFixture.make(canvas: 256)
  let r = try AvatarRenderer(package: made.package, images: made.images, maxSide: 256)
  // Each part's triangles, once for every layer mixed and once to lay the part: skin and lips one
  // drawing each, and an inner mouth only once the drawing has a picture of its own.
  let perPart = 35 * 23 * 2
  // What a triangle costs is the pixels it covers, not its box, so the counts are the areas the
  // mouth covers: at 256 pixels the skin is about 28 by 18 pixels when the mouth is open, and a
  // thin slit at rest. The values are deterministic; a change of more than a tenth is a change of
  // what is drawn, or a return to the cost of a box (which would be many times more).
  func near(_ got: Int, _ want: Int) -> Bool { abs(got - want) * 10 <= want }
  _ = r.render(AvatarFixture.frame(weights: AvatarFixture.weights(.rest)))
  #expect(r.lastMouthWork.triangles == 2 * 2 * perPart)
  #expect(near(r.lastMouthWork.pixels, 136), "at rest: \(r.lastMouthWork.pixels)")
  _ = r.render(AvatarFixture.frame(weights: AvatarFixture.weights(.a)))
  #expect(r.lastMouthWork.triangles == 3 * 2 * perPart)
  #expect(near(r.lastMouthWork.pixels, 1264), "open: \(r.lastMouthWork.pixels)")
  _ = r.render(AvatarFixture.frame(weights: AvatarFixture.weights(.a, .e, mix: 0.4)))
  #expect(r.lastMouthWork.triangles > 3 * 2 * perPart)
  #expect(near(r.lastMouthWork.pixels, 1560), "blend: \(r.lastMouthWork.pixels)")
}

@Test func aTriangleCostsItsCoveredPixelsAndNotItsBox() {
  // A thin diagonal sliver across a 200 pixel picture: its box is 40000 pixels, it covers a few.
  let sliver = (
    AvatarRasterizer.Vertex(x: 0, y: 0, u: 0, v: 0, alpha: 1),
    AvatarRasterizer.Vertex(x: 200, y: 200, u: 0, v: 0, alpha: 1),
    AvatarRasterizer.Vertex(x: 200, y: 198, u: 0, v: 0, alpha: 1)
  )
  let drawn = AvatarTestRaster.draw(
    [sliver], texture: [10, 10, 10, 255], size: (200, 200))
  let covered = drawn.covered
  let destination = drawn.picture
  #expect(covered > 0 && covered < 500)
  #expect(
    destination.enumerated().filter { $0.offset % 4 == 0 && $0.element != 0 }.count == covered)
}

@Test func aMeshThatCoversThePictureOverAndOverIsRefusedAtLoad() throws {
  // Two drawings of the inner mouth with no area at all (every corner of a drawing on one line)
  // blend into triangles of a canvas: the work of the mouth counts the box of each triangle's
  // corners over all nine drawings, so the load refuses it, once, and a frame never leaves the
  // mouth out (that would put the base picture's own mouth in its place).
  let made = try AvatarFixture.make(canvas: 64)
  var document = try JSONDecoder().decode(JsonValue.self, from: made.json)
  guard case .object(var fields) = document, case .object(var mouth)? = fields["mouth"],
    case .array(var parts)? = mouth["parts"], case .object(var part) = parts[0],
    case .array(var shapes)? = part["shapes"]
  else { throw AvatarVector.Unreadable(what: "the fixture's skin") }
  shapes = shapes.enumerated().map { index, shape in
    guard case .object(var one) = shape, case .array(let positions)? = one["positions"] else {
      return shape
    }
    // Drawing 0 on the diagonal, drawing 1 on the other: each has no area, and they blend into
    // triangles that span the picture.
    let side = Double(made.canvas)
    var points: [JsonValue] = []
    for i in 0..<(positions.count / 2) {
      let t = Double(i % 7) / 6 * side
      points.append(.number(index == 1 ? side - t : t))
      points.append(.number(t))
    }
    one["positions"] = .array(points)
    return .object(one)
  }
  part["shapes"] = .array(shapes)
  parts[0] = .object(part)
  mouth["parts"] = .array(parts)
  fields["mouth"] = .object(mouth)
  document = .object(fields)
  let assets = made.assets
  #expect(throws: AvatarPackageError(kind: .tooLarge, at: "/mouth/parts")) {
    _ = try AvatarPackage.load(json: JSONEncoder().encode(document), assets: assets)
  }
  // The same package with its mouth left alone loads.
  _ = try AvatarPackage.load(json: made.json, assets: assets)
}
