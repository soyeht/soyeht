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
private func worstStepOverEvenStep(inner: AvatarFixture.Inner) throws -> (
  ratio: Double, pair: String
) {
  let made = try AvatarFixture.make(canvas: 96, columns: 18, rows: 12, inner: inner)
  let r = try AvatarRenderer(package: made.package, images: made.images, maxSide: 96)
  let steps = 8
  var worst = (ratio: 0.0, pair: "")
  for a in AvatarShape.allCases {
    for b in AvatarShape.allCases where b.rawValue > a.rawValue {
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
  // eight steps would be a ratio of eight; five is the slack for an edge moving across a pixel
  // (the worst pair is under 4.5).
  let textured = try worstStepOverEvenStep(inner: .textured)
  #expect(textured.ratio < 5, "\(textured.pair): \(textured.ratio)")
  let collapsed = try worstStepOverEvenStep(inner: .collapsed)
  #expect(collapsed.ratio < 5, "\(collapsed.pair): \(collapsed.ratio)")
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

@Test func aDrawingThatBarelyTakesPartBarelyChangesThePicture() throws {
  let textured = try changeAtTheThreshold(inner: .textured)
  let collapsed = try changeAtTheThreshold(inner: .collapsed)
  #expect(textured < 8 && collapsed < 8)
}

@Test func anInnerMouthWithAHeightAndNoPictureAtRestPopsInWhole() throws {
  // A known problem of the shared rule, not of this renderer: the first textured drawing of a
  // part is drawn with the opacity 1 whatever its weight (`AvatarMeshPart.layers`, as in the Rust
  // crate), so an inner mouth that has a height where it has no picture appears whole as soon
  // as a drawing with a picture passes 0.0001. A real mesh has no height there. This marks the
  // rule for the follow-up: when it changes, this test starts to pass and must be removed.
  let change = try changeAtTheThreshold(inner: .untexturedAtRest)
  withKnownIssue { #expect(change < 8) }
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
  // Each part's triangles, once for every layer drawn: skin and lips one drawing each, and an
  // inner mouth only once the drawing has a picture of its own.
  let perPart = 35 * 23 * 2
  _ = r.render(AvatarFixture.frame(weights: AvatarFixture.weights(.rest)))
  #expect(r.lastMouthWork.triangles == 2 * perPart)
  let atRest = r.lastMouthWork.pixels
  _ = r.render(AvatarFixture.frame(weights: AvatarFixture.weights(.a)))
  #expect(r.lastMouthWork.triangles == 3 * perPart)
  _ = r.render(AvatarFixture.frame(weights: AvatarFixture.weights(.a, .e, mix: 0.4)))
  #expect(r.lastMouthWork.triangles > 3 * perPart)
  // What a triangle costs is the pixels it covers, not its box: the work is of the order of the
  // area of the mouth, here a few thousand pixels at a time (about 36 by 22 pixels of skin).
  #expect(atRest > 0 && atRest < 3_000)
  #expect(r.lastMouthWork.pixels < 20_000)
}

@Test func aTriangleCostsItsCoveredPixelsAndNotItsBox() {
  // A thin diagonal sliver across a 200 pixel picture: its box is 40000 pixels, it covers a few.
  let sliver = (
    AvatarRasterizer.Vertex(x: 0, y: 0, u: 0, v: 0, alpha: 1),
    AvatarRasterizer.Vertex(x: 200, y: 200, u: 0, v: 0, alpha: 1),
    AvatarRasterizer.Vertex(x: 200, y: 198, u: 0, v: 0, alpha: 1)
  )
  var destination = [UInt8](repeating: 0, count: 200 * 200 * 4)
  let texture: [UInt8] = [10, 10, 10, 255]
  var covered = 0
  texture.withUnsafeBufferPointer { tex in
    destination.withUnsafeMutableBufferPointer { dst in
      guard let tex = tex.baseAddress, let dst = dst.baseAddress else { return }
      covered = AvatarRasterizer.fill(
        sliver.0, sliver.1, sliver.2, opacity: 1, texture: tex, textureSize: (1, 1),
        destination: dst, destinationSize: (200, 200))
    }
  }
  #expect(covered > 0 && covered < 500)
  #expect(
    destination.enumerated().filter { $0.offset % 4 == 0 && $0.element != 0 }.count == covered)
}

@Test func aMeshThatCoversThePictureOverAndOverIsRefusedAtLoadAndNeverDrawnWithoutItsMouth() throws
{
  // Every vertex in a corner of the picture, so that each triangle covers half of it: the vertex
  // and triangle counts allow it, and a frame of nine drawings of it would cost seconds. The
  // renderer refuses it as too large when it is made, once, and a frame never leaves the mouth
  // out (that would put the base picture's own mouth in its place, a swap of drawings).
  let made = try AvatarFixture.make(canvas: 64)
  var package = made.package
  let side = Double(package.canvas.width)
  for part in package.mouth.parts.indices {
    for shape in package.mouth.parts[part].shapes.indices {
      let count = package.mouth.parts[part].shapes[shape].positions.count / 2
      var corners: [Double] = []
      for i in 0..<count {
        // The three corners in turn along a row and two along a column, so that the three
        // vertices of every triangle of the grid are three different corners.
        let corner = (i % 36 + 2 * (i / 36)) % 3
        corners.append(corner == 1 ? side : 0)
        corners.append(corner == 2 ? side : 0)
      }
      package.mouth.parts[part].shapes[shape].positions = corners
    }
  }
  #expect(throws: AvatarPackageError(kind: .tooLarge, at: "/mouth/parts/0/shapes/0/positions")) {
    _ = try AvatarRenderer(package: package, images: made.images, maxSide: 64)
  }
  // A drawing whose triangles cover exactly the canvas is within the rule.
  _ = try AvatarRenderer(package: made.package, images: made.images, maxSide: 64)
}
