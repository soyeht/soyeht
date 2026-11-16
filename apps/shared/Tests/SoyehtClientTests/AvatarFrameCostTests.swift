import Foundation
import Testing

@testable import SoyehtClient

/// What a frame costs: the runtime stepped and the picture drawn, at the size the stage uses (a
/// 1024-pixel canvas drawn at 512, a mouth mesh of a few thousand triangles, one core). The
/// numbers are printed so a run on another machine can be read, and the budget catches a change
/// that makes the rasteriser several times slower.
@Test func aFrameAtTheStageSizeCostsAFractionOfAFrameOfTheScreen() throws {
  let made = try AvatarFixture.make(canvas: 1024)
  let renderer = try AvatarRenderer(package: made.package, images: made.images, maxSide: 512)
  var runtime = AvatarRuntime(package: made.package, tune: AvatarTune(), seed: 3)
  let states: [AvatarState] = [.idle, .listening, .thinking, .speaking]
  var frames: [Double] = []
  var drawing: [Double] = []
  // The time this thread spent computing, not the time on the wall: a test run beside others
  // (or a loaded machine) must not fail for the time it waited.
  func now() -> Double { Double(clock_gettime_nsec_np(CLOCK_THREAD_CPUTIME_ID)) / 1e6 }
  for i in 0..<28 {
    let speaking = i % 40 > 8
    let audio =
      speaking ? AvatarAudio(position: Double(i) / 60, db: -30 + 18 * sin(Double(i) / 3)) : nil
    let start = now()
    let frame = runtime.step(
      dt: 1.0 / 60, state: speaking ? .speaking : states[(i / 20) % 3], audio: audio)
    let drawn = now()
    let picture = renderer.render(frame)
    let end = now()
    #expect(picture.width == 512 && picture.height == 512)
    // The first frames warm the buffers.
    if i >= 4 {
      frames.append(end - start)
      drawing.append(end - drawn)
    }
  }
  let median = frames.sorted()[frames.count / 2]
  let slowest = frames.sorted()[frames.count * 95 / 100]
  let render = drawing.sorted()[drawing.count / 2]
  print(
    "avatar frame at 512 px (CPU time): median \(median) ms, p95 \(slowest) ms (picture alone \(render) ms)"
  )
  #if DEBUG
    // An unoptimised build: a loose bound that still catches a quadratic mistake.
    #expect(median < 1500)
  #else
    #expect(median < 12)
  #endif
}
