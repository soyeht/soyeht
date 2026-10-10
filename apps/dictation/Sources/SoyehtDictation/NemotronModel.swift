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

  /// Written after a complete download and load, so a download that was cut short does not
  /// count as installed.
  private static let installedMarker = ".soyeht-installed"
  /// `fromPretrained` spends the first 70% of its progress on the download and the rest on
  /// compiling the CoreML models.
  private static let downloadShare = 0.7

  private let lock = NSLock()
  private var model: NemotronStreamingASRModel?

  var current: NemotronStreamingASRModel? { lock.withLock { model } }

  private let location: ModelCacheLocation

  init(
    location: ModelCacheLocation = ModelCacheLocation(
      modelId: NemotronStreamingASRModel.defaultModelId)
  ) {
    self.location = location
  }

  var isInstalled: Bool {
    FileManager.default.fileExists(
      atPath: location.directory.appendingPathComponent(Self.installedMarker).path)
  }

  /// Loads the installed model from disk, or downloads it first when it is not installed. The
  /// app only reaches the download branch from the Install button.
  func install(report: @escaping @Sendable (DictationModelStage) -> Void) async throws {
    let directory = location.directory
    let marker = directory.appendingPathComponent(Self.installedMarker)
    let loaded: NemotronStreamingASRModel
    if FileManager.default.fileExists(atPath: marker.path) {
      loaded = try await NemotronStreamingASRModel.fromLocal(
        bundleDir: directory, computeUnits: .all
      ) { _, _ in report(.preparing) }
    } else {
      loaded = try await NemotronStreamingASRModel.fromPretrained(computeUnits: .all) {
        fraction, _ in
        report(
          fraction >= Self.downloadShare ? .preparing : .downloading(fraction / Self.downloadShare))
      }
      try Data().write(to: marker)
    }
    lock.withLock { model = loaded }
  }

  /// Drops the loaded model and deletes the files and the folders around them, which frees the
  /// disk and leaves nothing behind.
  func remove() throws {
    lock.withLock { model = nil }
    try location.removeAll()
  }
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
