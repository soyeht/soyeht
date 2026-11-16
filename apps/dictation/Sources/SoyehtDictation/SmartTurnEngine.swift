import AudioCommon
import CoreML
import Foundation
import SoyehtClient

/// Smart Turn v3.2 as the CoreML model of `aufklarer/Smart-Turn-v3.2-CoreML`: from the last eight
/// seconds of the person's turn, the probability that they have finished, read from the sound of it
/// (pace and fall in pitch) in 23 languages. It is opt-in, after a one-time download the person
/// starts, and it sends nothing anywhere.
public enum SmartTurnEngine {
  /// The repository's `LICENSE` and the model card state BSD-2-Clause for the weights.
  static let license = ProviderLicense(
    name: "BSD-2-Clause", url: URL(string: "https://opensource.org/license/bsd-2-clause"),
    commercialUse: true)

  public static let provider = EndOfTurnProvider(
    id: "smart-turn", name: "Smart Turn",
    summary:
      "Tells a pause to think from the end of a turn, on this device. Nothing is sent anywhere.",
    platforms: Set(DictationPlatform.allCases),
    license: license,
    model: DictationModelInfo(
      sizeDescription: "about 17 MB", licenseName: license.name, licenseURL: license.url,
      commercialUse: license.commercialUse == true,
      install: { report in try await SmartTurnModelHolder.shared.install(report: report) },
      isInstalled: { SmartTurnModelHolder.shared.isInstalled },
      remove: { try SmartTurnModelHolder.shared.remove() }),
    availability: { LocalModelAvailability.appleSiliconWithEnoughMemory() },
    make: { SmartTurnModelHolder.shared.current.map(SmartTurnEndOfTurn.init) })
}

/// The Smart Turn model: its files on disk and the copy loaded in this process, like Kokoro's.
final class SmartTurnModelHolder: LocalModelHolder<MLModel>, @unchecked Sendable {
  static let shared = SmartTurnModelHolder()

  static let modelId = "aufklarer/Smart-Turn-v3.2-CoreML"
  private static let compiledModel = "smart_turn.mlmodelc"

  init(location: ModelCacheLocation = ModelCacheLocation(modelId: SmartTurnModelHolder.modelId)) {
    super.init(location: location) { installed, directory, report in
      try await HuggingFaceDownloader.downloadFiles(
        modelId: Self.modelId, to: directory,
        files: ["\(Self.compiledModel)/**", "config.json"], offlineMode: installed
      ) { report(.downloading($0)) }
      report(.preparing)
      let configuration = MLModelConfiguration()
      configuration.computeUnits = .cpuAndNeuralEngine
      return try MLModel(
        contentsOf: directory.appendingPathComponent(Self.compiledModel, isDirectory: true),
        configuration: configuration)
    }
  }
}

/// The end-of-turn check of one turn.
final class SmartTurnEndOfTurn: EndOfTurnDetecting, @unchecked Sendable {
  /// The model's window: eight seconds at 16 kHz, with the newest audio last.
  static let window = VoiceFrames.turnWindow
  /// The turn is complete above this probability (the model's `config.json`).
  static let threshold = 0.5

  private static let queue = DispatchQueue(label: "com.soyeht.voice.smartturn")
  private let model: MLModel

  init(_ model: MLModel) { self.model = model }

  /// A model that fails to answer is no reason to hold the person's turn: the pause decides.
  func isComplete(turn audio: [Float]) async -> Bool {
    do {
      return try await probability(of: audio) > Self.threshold
    } catch {
      ClientDiagnostics.reportOnce("voice_smart_turn_failed", key: "voice", error: error)
      return true
    }
  }

  private func probability(of audio: [Float]) async throws -> Double {
    try await withCheckedThrowingContinuation { continuation in
      Self.queue.async { [model] in
        continuation.resume(with: Result { try Self.run(model, on: audio) })
      }
    }
  }

  static func run(_ model: MLModel, on audio: [Float]) throws -> Double {
    let input = try MLMultiArray(shape: [1, NSNumber(value: window)], dataType: .float32)
    let tail = audio.suffix(window)
    let offset = window - tail.count
    input.withUnsafeMutableBytes { raw, _ in
      let samples = raw.bindMemory(to: Float.self)
      for index in 0..<offset { samples[index] = 0 }
      for (index, sample) in tail.enumerated() { samples[offset + index] = sample }
    }
    let output = try model.prediction(
      from: MLDictionaryFeatureProvider(dictionary: ["audio": MLFeatureValue(multiArray: input)]))
    guard let result = output.featureValue(for: "probability")?.multiArrayValue else {
      throw SmartTurnFailure.noProbability
    }
    return result[0].doubleValue
  }

  enum SmartTurnFailure: LocalizedError {
    case noProbability

    var errorDescription: String? { "The turn model returned no probability." }
  }
}
