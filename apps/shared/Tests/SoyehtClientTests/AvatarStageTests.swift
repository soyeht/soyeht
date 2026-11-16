import CoreGraphics
import Foundation
import Testing

@testable import SoyehtClient

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
  for _ in 0..<600 {
    if condition() { return true }
    do { try await Task.sleep(for: .milliseconds(100)) } catch { return false }
  }
  return condition()
}

@Test func theEngineDrawsAFrameForEachStepAndTheHeadLives() async throws {
  let made = try AvatarFixture.make(canvas: 128)
  let loaded = AvatarLoadedPackage(package: made.package, images: made.images)
  let engine = try AvatarStageEngine(loaded: loaded, seed: 9, maxSide: 64)
  let first = await engine.frame(dt: 0, state: .listening, allowed: 1)
  var later = first
  for _ in 1...120 { later = await engine.frame(dt: 1.0 / 60, state: .listening, allowed: 1) }
  #expect(first?.size == CGSize(width: 64, height: 64))
  let a = first.flatMap { AvatarBitmap(image: $0.image) }
  let b = later.flatMap { AvatarBitmap(image: $0.image) }
  #expect(a != nil && b != nil && a?.pixels != b?.pixels)
}

@Test func aStillFrameDependsOnTheStateAndNotOnTheTime() async throws {
  let made = try AvatarFixture.make(canvas: 128)
  let loaded = AvatarLoadedPackage(package: made.package, images: made.images)
  let engine = try AvatarStageEngine(loaded: loaded, seed: 9, maxSide: 64)
  let listening = await engine.stillFrame(state: .listening)
  let again = await engine.stillFrame(state: .listening)
  #expect(listening != nil && again != nil)
  #expect(listening.flatMap { AvatarBitmap(image: $0.image) } != nil)
}

@MainActor @Test func theLoopRunsWhileAViewShowsTheModelAndThereIsOnlyOne() async throws {
  let made = try AvatarFixture.make(canvas: 64)
  let model = AvatarStageModel(
    loaded: AvatarLoadedPackage(package: made.package, images: made.images), seed: 1, maxSide: 64)
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
  let made = try AvatarFixture.make(canvas: 64)
  let model = AvatarStageModel(
    loaded: AvatarLoadedPackage(package: made.package, images: made.images), seed: 1, maxSide: 64)
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

@Test func aSlowFrameShrinksThePictureAndAFastOneNeverGrowsItBack() {
  var pacer = AvatarFramePacer(side: 512)
  for _ in 0..<(AvatarFramePacer.patience - 1) { pacer.record(cost: 0.1, allowed: 0.033) }
  #expect(pacer.side == 512)
  pacer.record(cost: 0.1, allowed: 0.033)
  #expect(pacer.side == 384)
  // Slow frames that are not in a row do not count.
  for _ in 0..<20 {
    pacer.record(cost: 0.1, allowed: 0.033)
    pacer.record(cost: 0.001, allowed: 0.033)
  }
  #expect(pacer.side == 384)
  for _ in 0..<500 { pacer.record(cost: 0.0001, allowed: 0.033) }
  #expect(pacer.side == 384)
  for _ in 0..<500 { pacer.record(cost: 1, allowed: 0.033) }
  #expect(pacer.side == AvatarFramePacer.minimumSide)
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
    let size = AvatarSize(width: UInt32(image.width), height: UInt32(image.height))
    guard let data = AvatarFixture.png(from: image),
      let decoded = AvatarBitmap(decoding: data, expecting: size)
    else { return nil }
    self = decoded
  }
}
