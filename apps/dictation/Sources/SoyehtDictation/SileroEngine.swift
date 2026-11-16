import Foundation
import SoyehtClient
@preconcurrency import SpeechVAD

/// Silero VAD as the CoreML model of `aufklarer/Silero-VAD-v6.2.1-CoreML`: the speech probability
/// of every 32 ms of microphone audio, on this device. It is opt-in, after a one-time download the
/// person starts, and it sends nothing anywhere.
///
/// ADR 0166 names Silero VAD v5; the CoreML conversion this module's dependency carries is of the
/// same model's later 6.2.1 weights, under the same MIT license.
public enum SileroEngine {
  /// The model card states MIT, and so does the license of the Silero project.
  static let license = ProviderLicense(
    name: "MIT", url: URL(string: "https://opensource.org/license/mit"), commercialUse: true)

  public static let provider = VoiceActivityProvider(
    id: "silero", name: "Silero VAD",
    summary: "Tells a voice from other sounds, on this device. Nothing is sent anywhere.",
    platforms: Set(DictationPlatform.allCases),
    license: license,
    model: DictationModelInfo(
      sizeDescription: "about 2 MB", licenseName: license.name, licenseURL: license.url,
      commercialUse: license.commercialUse == true,
      install: { report in try await SileroModelHolder.shared.install(report: report) },
      isInstalled: { SileroModelHolder.shared.isInstalled },
      remove: { try SileroModelHolder.shared.remove() }),
    availability: { LocalModelAvailability.appleSiliconWithEnoughMemory() },
    make: { SileroModelHolder.shared.current.map(SileroVoiceActivity.init) })
}

/// The Silero model: its files on disk and the copy loaded in this process, like Kokoro's.
final class SileroModelHolder: LocalModelHolder<SileroVADModel>, @unchecked Sendable {
  static let shared = SileroModelHolder()

  /// `fromPretrained` spends the first 80% of its progress on the download and the rest on loading.
  private static let downloadShare = 0.8

  init(
    location: ModelCacheLocation = ModelCacheLocation(
      modelId: SileroVADModel.defaultCoreMLModelId)
  ) {
    super.init(location: location) { installed, directory, report in
      try await SileroVADModel.fromPretrained(
        engine: .coreml, cacheDir: directory, offlineMode: installed
      ) { fraction, _ in
        report(
          installed || fraction >= Self.downloadShare
            ? .preparing : .downloading(fraction / Self.downloadShare))
      }
    }
  }
}

/// One stream of microphone audio read by the model. The model carries its state from frame to
/// frame, is not safe for two threads at once, and serves one stream: the lock keeps a stray call
/// from another thread from corrupting it.
final class SileroVoiceActivity: VoiceActivityDetecting, @unchecked Sendable {
  private let lock = NSLock()
  private let model: SileroVADModel

  init(_ model: SileroVADModel) {
    self.model = model
    model.resetState()
  }

  func probability(of frame: [Float]) -> Double {
    guard frame.count == SileroVADModel.chunkSize else { return 0 }
    return lock.withLock { Double(model.processChunk(frame)) }
  }

  func reset() { lock.withLock { model.resetState() } }
}
