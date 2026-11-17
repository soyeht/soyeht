import Foundation
import SoyehtClient
import os

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

/// A model that downloads on request and loads into this process: where its files live, the copy
/// loaded, and the install the model catalogues call. Each model says only how it is loaded.
///
/// The model is loaded from disk with no network when it is installed, and downloaded first when it
/// is not: the app only reaches the download branch from the Install button.
class LocalModelHolder<Model>: @unchecked Sendable {
  /// Loads the model. `installed` says its files are already in `directory`, so nothing may be
  /// fetched; `report` tells the install how far it is.
  typealias Load =
    @Sendable (
      _ installed: Bool, _ directory: URL,
      _ report: @escaping @Sendable (DictationModelStage) -> Void
    ) async throws -> Model

  private let loaded = LoadedModel<Model>()
  private let location: ModelCacheLocation
  private let load: Load

  init(location: ModelCacheLocation, load: @escaping Load) {
    self.location = location
    self.load = load
  }

  var current: Model? { loaded.current }

  var isInstalled: Bool { location.isInstalled }

  /// The install that is under way, so a second call joins it: two installs of the same files would
  /// race on moving them into place.
  private let installing = OSAllocatedUnfairLock<Task<Void, any Error>?>(initialState: nil)

  func install(report: @escaping @Sendable (DictationModelStage) -> Void) async throws {
    let task = installing.withLock { running -> Task<Void, any Error> in
      if let running { return running }
      let started = Task { try await self.perform(report: report) }
      running = started
      return started
    }
    defer { installing.withLock { if $0 == task { $0 = nil } } }
    try await task.value
  }

  private func perform(report: @escaping @Sendable (DictationModelStage) -> Void) async throws {
    let installed = location.isInstalled
    let model = try await load(installed, location.directory, report)
    if !installed { try location.markInstalled() }
    loaded.set(model)
  }

  func remove() throws { try loaded.remove(files: location) }
}
