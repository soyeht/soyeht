import CoreGraphics
import Foundation
import Testing

@testable import SoyehtClient

/// A new folder that is removed when the test ends.
private func withFolder<T>(_ body: (URL) throws -> T) throws -> T {
  let folder = FileManager.default.temporaryDirectory
    .appendingPathComponent("avatar-test-\(UUID().uuidString)")
  try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
  defer { try? FileManager.default.removeItem(at: folder) }
  return try body(folder)
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

@Test func aPackageFolderLoadsAndDrawsAtTheSizeOfTheStage() throws {
  let made = try AvatarFixture.make(canvas: 128)
  try withFolder { folder in
    try AvatarFixture.write(made, to: folder)
    let loaded = try AvatarPackageFolder.load(folder)
    #expect(loaded.package == made.package)
    #expect(loaded.images.count == made.package.files.count)
    let renderer = try loaded.renderer(maxSide: 64)
    #expect(renderer.outputWidth == 64 && renderer.outputHeight == 64)
  }
}

@Test func theStageDrawsAFrameForEachStepAndTheHeadLives() async throws {
  let made = try AvatarFixture.make(canvas: 128)
  let loaded = AvatarLoadedPackage(package: made.package, images: made.images)
  let model = try await MainActor.run { try AvatarStageModel(loaded: loaded, seed: 9, maxSide: 64) }
  let (first, later, size) = await MainActor.run {
    let first = model.image
    model.state = .listening
    for i in 1...120 { model.advance(to: Double(i) / 60) }
    return (first, model.image, model.pictureSize)
  }
  #expect(size == CGSize(width: 64, height: 64))
  #expect(first?.width == 64 && later?.height == 64)
  let a = first.flatMap { AvatarBitmap(image: $0) }
  let b = later.flatMap { AvatarBitmap(image: $0) }
  #expect(a != nil && b != nil && a?.pixels != b?.pixels)
}

@Test func aFolderWithNoDocumentIsMalformed() throws {
  try withFolder { folder in
    #expect(error { try AvatarPackageFolder.load(folder) } == .malformed)
  }
}

@Test func aLinkInsteadOfAPictureIsNotFollowed() throws {
  let made = try AvatarFixture.make(canvas: 64)
  try withFolder { folder in
    try AvatarFixture.write(made, to: folder)
    let outside = folder.appendingPathComponent("outside.png")
    try FileManager.default.moveItem(at: folder.appendingPathComponent("base.png"), to: outside)
    try FileManager.default.createSymbolicLink(
      at: folder.appendingPathComponent("base.png"), withDestinationURL: outside)
    #expect(error { try AvatarPackageFolder.load(folder) } == .missingAsset)
  }
}

@Test func aPictureOfAnotherSizeThanTheDocumentSaysIsRefused() throws {
  let made = try AvatarFixture.make(canvas: 64)
  try withFolder { folder in
    try AvatarFixture.write(made, to: folder)
    let wrong = AvatarFixture.bitmap(10, 10) { _, _ in [1, 2, 3, 255] }
    try AvatarFixture.png(wrong).write(to: folder.appendingPathComponent("atlas.png"))
    #expect(error { try AvatarPackageFolder.load(folder) } == .assetSize)
  }
}

@Test func aFileThatIsNotAPictureIsRefused() throws {
  let made = try AvatarFixture.make(canvas: 64)
  try withFolder { folder in
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
