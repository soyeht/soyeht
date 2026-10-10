import AudioCommon
import Foundation
import Testing

@testable import SoyehtDictation

private let modelId = "aufklarer/Test-Model"

/// Runs `body` with a fresh temporary folder and a cache location inside it, so no test touches
/// the real caches. The folder is deleted afterwards.
private func inScratchCache(
  _ body: (_ root: URL, _ location: ModelCacheLocation) throws -> Void
) throws {
  let root = FileManager.default.temporaryDirectory
    .appendingPathComponent("model-cache-\(UUID().uuidString)", isDirectory: true)
  try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
  defer { try? FileManager.default.removeItem(at: root) }
  let base = root.appendingPathComponent("qwen3-speech", isDirectory: true)
  try body(root, ModelCacheLocation(modelId: modelId, base: base))
}

private func exists(_ url: URL) -> Bool { FileManager.default.fileExists(atPath: url.path) }

private func isEmpty(_ folder: URL) throws -> Bool {
  try FileManager.default.contentsOfDirectory(atPath: folder.path).isEmpty
}

private func make(_ url: URL) throws {
  try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
}

@Test func askingWhereTheModelLivesCreatesNothingOnDisk() throws {
  try inScratchCache { root, location in
    #expect(!NemotronModelHolder(location: location).isInstalled)
    let empty = try isEmpty(root)
    #expect(empty)
  }
}

@Test func theFolderIsTheOneTheDownloaderUses() throws {
  try inScratchCache { _, location in
    let downloaders = try HuggingFaceDownloader.getCacheDirectory(
      for: modelId, basePath: location.base)
    #expect(location.directory.standardizedFileURL == downloaders.standardizedFileURL)
  }
}

@Test func aModelAlreadyInTheOlderFlatFolderIsFoundThere() throws {
  try inScratchCache { _, location in
    let flat = location.base.appendingPathComponent(
      HuggingFaceDownloader.sanitizedCacheKey(for: modelId))
    try make(flat.appendingPathComponent("encoder.mlmodelc"))

    let downloaders = try HuggingFaceDownloader.getCacheDirectory(
      for: modelId, basePath: location.base)
    #expect(location.directory.standardizedFileURL == flat.standardizedFileURL)
    #expect(location.directory.standardizedFileURL == downloaders.standardizedFileURL)
  }
}

@Test func removingAnInstalledModelDeletesItsFolderAndTheEmptyOnesAroundIt() throws {
  try inScratchCache { root, location in
    let holder = NemotronModelHolder(location: location)
    try make(location.directory.appendingPathComponent("encoder.mlmodelc"))
    try Data().write(to: location.directory.appendingPathComponent(".soyeht-installed"))
    #expect(holder.isInstalled)

    try holder.remove()

    #expect(!holder.isInstalled)
    #expect(!exists(location.directory))
    #expect(!exists(location.base))
    #expect(exists(root))
  }
}

@Test func removingKeepsAFolderAnotherModelStillUses() throws {
  try inScratchCache { _, location in
    let other = location.base.appendingPathComponent("models/aufklarer/Other-Model")
    try make(other)
    try make(location.directory)

    try location.removeAll()

    #expect(!exists(location.directory))
    #expect(exists(other))
  }
}

@Test func removingAModelThatWasNeverDownloadedDoesNothing() throws {
  try inScratchCache { root, location in
    try NemotronModelHolder(location: location).remove()
    let empty = try isEmpty(root)
    #expect(empty)
  }
}
