import AudioCommon
import Foundation

/// Where the downloader keeps one model on disk, worked out without touching the disk.
///
/// The downloader's own `getCacheDirectory` creates the folder it returns, so asking it whether
/// a model is installed would leave an empty folder behind for a person who never pressed
/// Install. This type mirrors its layout instead, and a test compares the two. Nothing is
/// created here: only the downloader does that, once the person has agreed to the download.
struct ModelCacheLocation: Sendable {
  let modelId: String
  /// The downloader's cache root, `qwen3-speech` inside the caches folder or inside the folder
  /// named by `QWEN3_CACHE_DIR` / `QWEN3_ASR_CACHE_DIR`.
  let base: URL

  init(modelId: String, base: URL = Self.defaultBase()) {
    self.modelId = modelId
    self.base = base
  }

  /// The older flat layout, `aufklarer_Model`, which the downloader keeps using while it still
  /// holds the weights.
  private var flat: URL {
    base.appendingPathComponent(HuggingFaceDownloader.sanitizedCacheKey(for: modelId))
  }

  /// The Hub layout, `models/aufklarer/Model`.
  private var hub: URL {
    base.appendingPathComponent("models", isDirectory: true)
      .appendingPathComponent(modelId, isDirectory: true)
  }

  /// The folder the downloader reads and writes. It may not exist yet.
  var directory: URL {
    HuggingFaceDownloader.weightsExist(in: flat) ? flat : hub
  }

  /// Deletes every folder this model could live in, then the empty folders the downloader made
  /// around it, so the cache root is gone too when no other model uses it. Does nothing for a
  /// model that was never downloaded.
  func removeAll(fileManager: FileManager = .default) throws {
    for folder in [flat, hub] where fileManager.fileExists(atPath: folder.path) {
      try fileManager.removeItem(at: folder)
    }
    var parent = hub.deletingLastPathComponent()
    while parent.path.hasPrefix(base.path), Self.isEmptyDirectory(parent, fileManager) {
      try fileManager.removeItem(at: parent)
      parent.deleteLastPathComponent()
    }
  }

  private static func isEmptyDirectory(_ url: URL, _ fileManager: FileManager) -> Bool {
    // A folder that cannot be listed is kept: the cleanup only removes what it can see is empty.
    (try? fileManager.contentsOfDirectory(atPath: url.path))?.isEmpty == true
  }

  private static func defaultBase() -> URL {
    let environment = ProcessInfo.processInfo.environment
    let override = ["QWEN3_CACHE_DIR", "QWEN3_ASR_CACHE_DIR"]
      .compactMap { environment[$0]?.trimmingCharacters(in: .whitespacesAndNewlines) }
      .first { !$0.isEmpty }
    let root =
      override.map { URL(fileURLWithPath: $0, isDirectory: true) }
      ?? FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first
      ?? FileManager.default.temporaryDirectory
    return root.appendingPathComponent("qwen3-speech", isDirectory: true)
  }
}
