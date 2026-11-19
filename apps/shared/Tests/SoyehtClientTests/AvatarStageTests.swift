import CoreGraphics
import Foundation
import SoyehtAvatarRaster
import Testing
import os

@testable import SoyehtAvatar
@testable import SoyehtClient

extension AvatarStageSettings {
  /// A stage of a small picture, so a test draws fast.
  fileprivate static let small = AvatarStageSettings(
    logger: Logger(subsystem: "com.soyeht.next.tests", category: "avatar"), maxSide: 64)
}

/// A stage on the small test package, seeded so its pictures repeat.
@MainActor private func smallModel() throws -> AvatarStageModel {
  let made = try AvatarFixture.make(canvas: 64)
  return AvatarStageModel(
    loaded: AvatarLoadedPackage(package: made.package, images: made.images), seed: 1,
    settings: .small)
}

/// A new folder that is removed when the test ends.
private func withFolder<T>(
  isolation: isolated (any Actor)? = #isolation, _ body: (URL) async throws -> T
) async throws -> T {
  let folder = FileManager.default.temporaryDirectory
    .appendingPathComponent("avatar-test-\(UUID().uuidString)")
  try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
  defer { try? FileManager.default.removeItem(at: folder) }
  return try await body(folder)
}

private func error(_ body: () throws -> Any) -> AvatarPackageErrorKind? {
  do {
    _ = try body()
    return nil
  } catch let failure as AvatarPackageError {
    return failure.kind
  } catch {
    return .malformed
  }
}

@Test func aPackageFolderLoadsAndDrawsAtTheSizeOfTheStage() async throws {
  let made = try AvatarFixture.make(canvas: 128)
  try await withFolder { folder in
    try AvatarFixture.write(made, to: folder)
    let loaded = try AvatarPackageFolder.load(folder)
    #expect(loaded.package == made.package)
    #expect(loaded.images.count == made.package.files.count)
    let renderer = try loaded.renderer(maxSide: 64)
    #expect(renderer.outputWidth == 64 && renderer.outputHeight == 64)
  }
}

/// Wait for a condition of the main actor, polling, up to a minute (a debug build is slow).
@MainActor private func eventually(_ condition: () -> Bool) async -> Bool {
  for _ in 0..<6000 {
    if condition() { return true }
    do { try await Task.sleep(for: .milliseconds(10)) } catch { return false }
  }
  return condition()
}

@Test func theEngineDrawsAFrameForEachStepAndTheHeadLives() async throws {
  let made = try AvatarFixture.make(canvas: 128)
  let loaded = AvatarLoadedPackage(package: made.package, images: made.images)
  let engine = try AvatarStageEngine(loaded: loaded, seed: 9, settings: .small)
  let first = await engine.frame(dt: 0, state: .listening, allowed: 1)
  var later = first
  for _ in 1...120 { later = await engine.frame(dt: 1.0 / 60, state: .listening, allowed: 1) }
  #expect(first?.size == CGSize(width: 64, height: 64))
  let a = first.flatMap { AvatarBitmap(image: $0.image) }
  let b = later.flatMap { AvatarBitmap(image: $0.image) }
  #expect(a != nil && b != nil && a?.pixels != b?.pixels)
}

@Test func aStillFrameIsTheFaceOfItsStateAndOfNoOther() async throws {
  func still(_ state: AvatarState) async throws -> [UInt8] {
    let made = try AvatarFixture.make(canvas: 128)
    let loaded = AvatarLoadedPackage(package: made.package, images: made.images)
    let engine = try AvatarStageEngine(loaded: loaded, seed: 9, settings: .small)
    let picture = try #require(await engine.stillFrame(state: state))
    return try #require(AvatarBitmap(image: picture.image)).pixels
  }
  // The same seed and state give the same picture; another state gives another one (the mouth
  // holds the listening smile or the thinking expression, and the head is turned).
  #expect(try await still(.listening) == still(.listening))
  #expect(try await still(.listening) != still(.thinking))
}

@MainActor @Test func theLoopRunsWhileAViewShowsTheModelAndThereIsOnlyOne() async throws {
  let model = try smallModel()
  let (one, two) = (UUID(), UUID())
  #expect(!model.isRunning)
  model.setViewing(one, true)
  model.setViewing(two, true)
  #expect(model.isRunning)
  #expect(await eventually { model.image != nil })
  #expect(model.pictureSize == CGSize(width: 64, height: 64))
  // One view leaves: the other still shows the model, and the loop goes on.
  model.setViewing(one, false)
  #expect(model.isRunning)
  // An inactive scene is a view that does not show the model.
  model.setViewing(two, false)
  #expect(!model.isRunning)
}

@MainActor @Test func withReduceMotionTheModelDrawsOnePicturePerState() async throws {
  let model = try smallModel()
  model.reducesMotion = true
  let viewer = UUID()
  model.setViewing(viewer, true)
  #expect(await eventually { model.image != nil })
  let still = model.image
  try await Task.sleep(for: .milliseconds(700))
  #expect(model.image === still)
  model.state = .thinking
  #expect(await eventually { model.image !== still })
  model.setViewing(viewer, false)
}

@MainActor @Test func aFolderIsReadWhenTheFirstViewAppears() async throws {
  let made = try AvatarFixture.make(canvas: 64)
  let (shown, failure) = try await withFolder { folder -> (Bool, AvatarPackageErrorKind?) in
    try AvatarFixture.write(made, to: folder)
    let model = AvatarStageModel(folder: folder)
    let viewer = UUID()
    model.setViewing(viewer, true)
    let shown = await eventually { model.image != nil }
    model.setViewing(viewer, false)
    let broken = AvatarStageModel(folder: folder.appendingPathComponent("missing"))
    broken.setViewing(viewer, true)
    let failed = await eventually { broken.failure != nil }
    broken.setViewing(viewer, false)
    return (shown && failed, broken.failure)
  }
  #expect(shown)
  #expect(failure == .malformed)
}

@Test func aSlowFrameShrinksThePicture() {
  var pacer = AvatarFramePacer(side: 512)
  var now = 0.0
  func record(_ cost: Double) -> AvatarFramePacer.Change? {
    now += 0.033
    return pacer.record(cost: cost, allowed: 0.033, now: now)
  }
  for _ in 0..<(AvatarFramePacer.patience - 1) { #expect(record(0.1) == nil) }
  #expect(record(0.1) == .shrank(to: 384))
  // Slow frames that are not in a row do not count.
  for _ in 0..<20 {
    #expect(record(0.1) == nil)
    #expect(record(0.03) == nil)
  }
  #expect(pacer.side == 384)
  for _ in 0..<500 { _ = record(1) }
  #expect(pacer.side == AvatarFramePacer.minimumSide)
}

@Test func aPictureGrowsBackAfterTenQuietSecondsAndTheWaitDoublesWhenItCannotStay() {
  var pacer = AvatarFramePacer(side: 400)
  var now = 0.0
  func frames(seconds: Double, cost: Double) -> [AvatarFramePacer.Change] {
    var changes: [AvatarFramePacer.Change] = []
    for _ in 0..<Int(seconds / 0.033) {
      now += 0.033
      if let change = pacer.record(cost: cost, allowed: 0.033, now: now) { changes.append(change) }
    }
    return changes
  }
  #expect(frames(seconds: 0.2, cost: 0.1) == [.shrank(to: 300)])
  // Under half of the allowance for under ten seconds changes nothing; a frame between half and
  // all of it starts the count again.
  #expect(frames(seconds: 9, cost: 0.001).isEmpty)
  #expect(frames(seconds: 0.2, cost: 0.02).isEmpty)
  #expect(frames(seconds: 9, cost: 0.001).isEmpty)
  #expect(frames(seconds: 1.5, cost: 0.001) == [.grew(to: 400)])
  // At the side it started at, it grows no more.
  #expect(frames(seconds: 3, cost: 0.001).isEmpty)
  // It shrinks again soon after it grew: the wait is now twenty seconds.
  #expect(frames(seconds: 0.2, cost: 0.1) == [.shrank(to: 300)])
  #expect(frames(seconds: 15, cost: 0.001).isEmpty)
  #expect(frames(seconds: 6, cost: 0.001) == [.grew(to: 400)])
  // A picture that costs 19 ms a frame (a debug build) fits an idle frame, 33 ms, but not a
  // speaking one, 16.7 ms: it is never quiet, so it does not grow while idle to shrink at the next
  // sentence.
  var pulsing = AvatarFramePacer(side: 400)
  now = 0
  _ = pulsing.record(cost: 0.1, allowed: 0.033, now: 0)
  var shrunk = false
  for _ in 0..<(AvatarFramePacer.patience + 1) {
    now += 0.033
    shrunk = pulsing.record(cost: 0.1, allowed: 0.033, now: now) != nil || shrunk
  }
  #expect(shrunk)
  let before = pulsing.side
  for _ in 0..<5_000 {
    now += 0.033
    #expect(pulsing.record(cost: 0.019, allowed: 0.033, now: now) == nil)
  }
  #expect(pulsing.side == before)
  // It started where the renderer was: a picture smaller than the request never grows past it.
  var small = AvatarFramePacer(side: 120)
  #expect(small.record(cost: 0, allowed: 0.033, now: 0) == nil)
  #expect(small.record(cost: 0, allowed: 0.033, now: 100) == nil)
  #expect(small.side == 120)
}

@Test func theQuietAllowanceIsTheShortestFrameOfTheStageNotAConstant() {
  // A frame of 40 ms, inside the 100 ms a slow stage allows, is quiet for a stage whose shortest
  // frame is 100 ms (under half of it) and never for the standard one (1/60 s).
  func grows(_ pacer: inout AvatarFramePacer) -> Bool {
    var now = 0.0
    for _ in 0..<AvatarFramePacer.patience {
      now += 0.1
      _ = pacer.record(cost: 0.2, allowed: 0.1, now: now)
    }
    guard pacer.side < pacer.ceiling else { return false }
    for _ in 0..<400 {
      now += 0.1
      if case .grew? = pacer.record(cost: 0.04, allowed: 0.1, now: now) { return true }
    }
    return false
  }
  var slow = AvatarFramePacer(side: 400, quietAllowance: 0.1)
  var standard = AvatarFramePacer(side: 400)
  #expect(grows(&slow))
  #expect(!grows(&standard))
}

@Test func aFolderWithNoDocumentIsMalformed() async throws {
  try await withFolder { folder in
    #expect(error { try AvatarPackageFolder.load(folder) } == .malformed)
  }
}

@Test func aLinkInsteadOfAPictureIsNotFollowed() async throws {
  let made = try AvatarFixture.make(canvas: 64)
  try await withFolder { folder in
    try AvatarFixture.write(made, to: folder)
    let outside = folder.appendingPathComponent("outside.png")
    try FileManager.default.moveItem(at: folder.appendingPathComponent("base.png"), to: outside)
    try FileManager.default.createSymbolicLink(
      at: folder.appendingPathComponent("base.png"), withDestinationURL: outside)
    #expect(error { try AvatarPackageFolder.load(folder) } == .missingAsset)
  }
}

@Test func aLinkedDirectoryInsideThePackageIsNotFollowed() async throws {
  let made = try AvatarFixture.make(canvas: 64)
  try await withFolder { folder in
    try AvatarFixture.write(made, to: folder)
    let elsewhere = folder.appendingPathComponent("elsewhere")
    try FileManager.default.moveItem(at: folder.appendingPathComponent("frames"), to: elsewhere)
    try FileManager.default.createSymbolicLink(
      at: folder.appendingPathComponent("frames"), withDestinationURL: elsewhere)
    #expect(error { try AvatarPackageFolder.load(folder) } == .missingAsset)
  }
}

@Test func aFileOverItsLimitIsTooLargeAndNotAMissingOrMalformedOne() async throws {
  let made = try AvatarFixture.make(canvas: 64)
  try await withFolder { folder in
    try AvatarFixture.write(made, to: folder)
    let big = Data(repeating: 0, count: Int(AvatarPackageLimits.fileBytes) + 1)
    try big.write(to: folder.appendingPathComponent("atlas.png"))
    #expect(error { try AvatarPackageFolder.load(folder) } == .tooLarge)
    try AvatarFixture.png(made.images["atlas.png"] ?? AvatarBitmap(width: 1, height: 1))
      .write(to: folder.appendingPathComponent("atlas.png"))
    try Data(repeating: 0x20, count: AvatarPackageLimits.documentBytes + 1)
      .write(to: folder.appendingPathComponent("avatar.json"))
    #expect(error { try AvatarPackageFolder.load(folder) } == .tooLarge)
    try made.json.write(to: folder.appendingPathComponent("avatar.json"))
    #expect(error { try AvatarPackageFolder.load(folder) } == nil)
  }
}

@Test func aDocumentThatIsALinkIsMalformed() async throws {
  let made = try AvatarFixture.make(canvas: 64)
  try await withFolder { folder in
    try AvatarFixture.write(made, to: folder)
    let moved = folder.appendingPathComponent("doc.json")
    try FileManager.default.moveItem(at: folder.appendingPathComponent("avatar.json"), to: moved)
    try FileManager.default.createSymbolicLink(
      at: folder.appendingPathComponent("avatar.json"), withDestinationURL: moved)
    #expect(error { try AvatarPackageFolder.load(folder) } == .malformed)
  }
}

@Test func aPictureOfAnotherSizeThanTheDocumentSaysIsRefused() async throws {
  let made = try AvatarFixture.make(canvas: 64)
  try await withFolder { folder in
    try AvatarFixture.write(made, to: folder)
    let wrong = AvatarFixture.bitmap(10, 10) { _, _ in [1, 2, 3, 255] }
    try AvatarFixture.png(wrong).write(to: folder.appendingPathComponent("atlas.png"))
    #expect(error { try AvatarPackageFolder.load(folder) } == .assetSize)
  }
}

@Test func aFileThatIsNotAPictureIsRefused() async throws {
  let made = try AvatarFixture.make(canvas: 64)
  try await withFolder { folder in
    try AvatarFixture.write(made, to: folder)
    try Data("not a png".utf8).write(to: folder.appendingPathComponent("atlas.png"))
    #expect(error { try AvatarPackageFolder.load(folder) } == .assetSize)
  }
}

@Test func theSizeOfAPngIsReadFromItsHeaderAndNothingElseIsAPng() {
  let png = AvatarFixture.png(AvatarFixture.bitmap(7, 5) { _, _ in [9, 9, 9, 255] })
  #expect(avatarPngSize(png) == AvatarSize(width: 7, height: 5))
  #expect(avatarPngSize(Data("not a png at all, not at all".utf8)) == nil)
  #expect(avatarPngSize(png.prefix(10)) == nil)
}

extension AvatarBitmap {
  /// A picture the system drew, in this format (for a test that reads what a view would show).
  init?(image: CGImage) {
    guard let data = AvatarFixture.png(from: image),
      let decoded = AvatarBitmap(decoding: data, width: image.width, height: image.height)
    else { return nil }
    self = decoded
  }
}

@Test func aNamedPipeInsteadOfAPictureDoesNotHangTheLoad() async throws {
  let made = try AvatarFixture.make(canvas: 64)
  try await withFolder { folder in
    try AvatarFixture.write(made, to: folder)
    let pipe = folder.appendingPathComponent("atlas.png")
    try FileManager.default.removeItem(at: pipe)
    #expect(mkfifo(pipe.path, 0o600) == 0)
    // Opening a pipe for reading waits for a writer: the load must not.
    #expect(error { try AvatarPackageFolder.load(folder) } == .missingAsset)
    let document = folder.appendingPathComponent("avatar.json")
    try FileManager.default.removeItem(at: document)
    #expect(mkfifo(document.path, 0o600) == 0)
    #expect(error { try AvatarPackageFolder.load(folder) } == .malformed)
  }
}

@MainActor @Test func aPackageThatDidNotLoadSaysWhichKindAndNothingElse() async throws {
  let made = try AvatarFixture.make(canvas: 64)
  let text = try await withFolder { folder -> String? in
    try AvatarFixture.write(made, to: folder)
    try Data("not a png".utf8).write(to: folder.appendingPathComponent("atlas.png"))
    let model = AvatarStageModel(folder: folder)
    let viewer = UUID()
    model.setViewing(viewer, true)
    _ = await eventually { model.failure != nil }
    model.setViewing(viewer, false)
    return model.failureText
  }
  #expect(text == "The avatar could not be loaded (assetSize)")
}

@MainActor @Test func aViewThatComesBackWhileThePackageIsLoadingWaitsForTheSameLoad() async throws {
  let model = try smallModel()
  let viewer = UUID()
  for _ in 0..<5 {
    model.setViewing(viewer, true)
    model.setViewing(viewer, false)
  }
  model.setViewing(viewer, true)
  #expect(await eventually { model.image != nil })
  #expect(model.loadsStarted == 1)
  model.setViewing(viewer, false)
}
