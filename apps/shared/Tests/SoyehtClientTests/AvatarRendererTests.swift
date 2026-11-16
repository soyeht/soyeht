import Foundation
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

@Test func theMouthMorphsInSmallSteps() throws {
  // The picture a mouth makes changes by a small, even amount as the weights go from one drawing
  // to another: no step is a jump between pictures.
  let r = try renderer(try made(), maxSide: canvas)
  let steps = 12
  let frames = (0...steps).map { i in
    r.render(
      AvatarFixture.frame(
        weights: AvatarFixture.weights(.rest, .a, mix: Double(i) / Double(steps))))
  }
  let changes = zip(frames, frames.dropFirst()).map { meanDifference($0, $1) }
  let total = meanDifference(frames[0], frames[steps])
  #expect(total > 0)
  for change in changes { #expect(change < 4 * total / Double(steps) + 0.05) }
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
