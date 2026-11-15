import AudioCommon
import CoreML
import Foundation
@preconcurrency import KokoroTTS
import SoyehtClient

/// The Kokoro model: its files on disk and the copy loaded in this process. The files live in the
/// downloader's cache outside the app bundle, and nothing is fetched until the person installs
/// the voice.
///
/// The model is not safe for concurrent inference: `KokoroInference` runs every call on one
/// serial queue.
final class KokoroModelHolder: @unchecked Sendable {
  static let shared = KokoroModelHolder()

  /// `fromPretrained` spends the first 70% of its progress on the download and the rest on
  /// loading the CoreML models.
  private static let downloadShare = 0.7

  /// The library recommends the Neural Engine, and documents the GPU as the way around builds
  /// where its compiler gives incorrect output for this model, "some iOS 26 builds". The Mac is
  /// measured with the Neural Engine; a phone uses the GPU until one is measured.
  private static let computeUnits: MLComputeUnits = {
    #if os(iOS)
      .cpuAndGPU
    #else
      .all
    #endif
  }()

  private let loaded = LoadedModel<KokoroTTSModel>()
  private let location: ModelCacheLocation

  init(
    location: ModelCacheLocation = ModelCacheLocation(modelId: KokoroTTSModel.defaultModelId)
  ) {
    self.location = location
  }

  var current: KokoroTTSModel? { loaded.current }

  var isInstalled: Bool { location.isInstalled }

  /// Loads the installed model from disk with no network, or downloads it first when it is not
  /// installed. The app only reaches the download branch from the Install button.
  func install(report: @escaping @Sendable (DictationModelStage) -> Void) async throws {
    let installed = location.isInstalled
    let model = try await KokoroTTSModel.fromPretrained(
      cacheDir: location.directory, offlineMode: installed, computeUnits: Self.computeUnits
    ) { fraction, _ in
      report(
        installed || fraction >= Self.downloadShare
          ? .preparing : .downloading(fraction / Self.downloadShare))
    }
    if !installed { try location.markInstalled() }
    loaded.set(model)
  }

  func remove() throws { try loaded.remove(files: location) }
}

/// The model's calls, one at a time.
enum KokoroInference {
  private static let queue = DispatchQueue(label: "com.soyeht.voice.kokoro")

  static func render(
    _ text: String, voice: String, speed: Float, in model: KokoroTTSModel
  ) async throws -> [Float] {
    try await withCheckedThrowingContinuation { continuation in
      queue.async {
        continuation.resume(
          with: Result {
            try model.synthesize(text: text, voice: voice, language: "en", speed: speed)
          })
      }
    }
  }
}
