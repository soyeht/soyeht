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
final class KokoroModelHolder: LocalModelHolder<KokoroTTSModel>, @unchecked Sendable {
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

  init(
    location: ModelCacheLocation = ModelCacheLocation(modelId: KokoroTTSModel.defaultModelId)
  ) {
    super.init(location: location) { installed, directory, report in
      try await KokoroTTSModel.fromPretrained(
        cacheDir: directory, offlineMode: installed, computeUnits: Self.computeUnits
      ) { fraction, _ in
        report(
          installed || fraction >= Self.downloadShare
            ? .preparing : .downloading(fraction / Self.downloadShare))
      }
    }
  }
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
