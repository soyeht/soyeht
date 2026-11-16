import AudioCommon
import Foundation
@preconcurrency import NemotronStreamingASR
import SoyehtClient

/// The Nemotron model: its files on disk and the copy loaded in this process. The weights live
/// in the downloader's cache outside the app bundle, so the app does not grow, and nothing is
/// fetched until the person installs the engine.
///
/// The model and its sessions are not safe for concurrent inference. `NemotronInference` runs
/// every call on one serial queue, so two windows with a dictation each cannot overlap.
final class NemotronModelHolder: @unchecked Sendable {
  static let shared = NemotronModelHolder()

  /// `fromPretrained` spends the first 70% of its progress on the download and the rest on
  /// compiling the CoreML models.
  private static let downloadShare = 0.7

  private let loaded = LoadedModel<NemotronStreamingASRModel>()

  var current: NemotronStreamingASRModel? { loaded.current }

  private let location: ModelCacheLocation

  init(
    location: ModelCacheLocation = ModelCacheLocation(
      modelId: NemotronStreamingASRModel.defaultModelId)
  ) {
    self.location = location
  }

  var isInstalled: Bool { location.isInstalled }

  /// Loads the installed model from disk, or downloads it first when it is not installed. The
  /// app only reaches the download branch from the Install button.
  func install(report: @escaping @Sendable (DictationModelStage) -> Void) async throws {
    let model: NemotronStreamingASRModel
    if location.isInstalled {
      model = try await NemotronStreamingASRModel.fromLocal(
        bundleDir: location.directory, computeUnits: .all
      ) { _, _ in report(.preparing) }
    } else {
      model = try await NemotronStreamingASRModel.fromPretrained(computeUnits: .all) {
        fraction, _ in
        report(
          fraction >= Self.downloadShare ? .preparing : .downloading(fraction / Self.downloadShare))
      }
      try location.markInstalled()
    }
    loaded.set(model)
  }

  func remove() throws { try loaded.remove(files: location) }
}

/// One streaming session, serialized.
final class NemotronInference: @unchecked Sendable {
  private static let queue = DispatchQueue(label: "com.soyeht.dictation.nemotron")
  private let session: StreamingSession

  init(_ session: StreamingSession) {
    self.session = session
  }

  /// The newest transcript so far after feeding `samples`, or nil when no chunk completed.
  func push(_ samples: [Float]) async throws -> String? {
    try await run { try $0.pushAudio(samples).last?.text }
  }

  /// Flushes the audio below one chunk and returns the closed transcript.
  func finalize() async throws -> String? {
    try await run { try $0.finalize().last?.text }
  }

  private func run(_ work: @escaping @Sendable (StreamingSession) throws -> String?) async throws
    -> String?
  {
    try await withCheckedThrowingContinuation { continuation in
      Self.queue.async { [session] in
        continuation.resume(with: Result { try work(session) })
      }
    }
  }
}
