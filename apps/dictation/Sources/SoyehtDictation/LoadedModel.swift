import Foundation

/// The copy of a model loaded in this process, readable from any thread.
final class LoadedModel<Model>: @unchecked Sendable {
  private let lock = NSLock()
  private var model: Model?

  var current: Model? { lock.withLock { model } }

  func set(_ loaded: Model) {
    lock.withLock { model = loaded }
  }

  /// Drops the loaded model and deletes the files and the folders around them, which frees the
  /// disk and leaves nothing behind.
  func remove(files location: ModelCacheLocation) throws {
    lock.withLock { model = nil }
    try location.removeAll()
  }
}
