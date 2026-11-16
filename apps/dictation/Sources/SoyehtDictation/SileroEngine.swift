import CoreML
import Foundation
import SoyehtClient

/// Silero VAD as the CoreML model of `aufklarer/Silero-VAD-v6.2.1-CoreML`, at a pinned revision: the
/// speech probability of every 32 ms of microphone audio, on this device. It is opt-in, after a
/// one-time download the person starts, and it sends nothing anywhere.
///
/// ADR 0166 names Silero VAD v5; the CoreML conversion that exists is of the same project's 6.2.1
/// weights, under the same MIT license. The model is run here rather than through a library, so a
/// prediction that fails is an error the loop reports and not a silence.
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
      sizeDescription: "about 1 MB", licenseName: license.name, licenseURL: license.url,
      commercialUse: license.commercialUse == true,
      install: { report in try await SileroModelHolder.shared.install(report: report) },
      isInstalled: { SileroModelHolder.shared.isInstalled },
      remove: { try SileroModelHolder.shared.remove() }),
    availability: { LocalModelAvailability.appleSiliconWithEnoughMemory() },
    make: { SileroModelHolder.shared.current.map(SileroVoiceActivity.init) })
}

/// The Silero model: its files on disk and the compiled model loaded in this process, like Kokoro's.
/// The compiled model holds no stream: each `SileroVoiceActivity` has its own state.
final class SileroModelHolder: LocalModelHolder<MLModel>, @unchecked Sendable {
  static let shared = SileroModelHolder()

  static let compiledModel = "silero_vad.mlmodelc"

  init(
    location: ModelCacheLocation = ModelCacheLocation(modelId: PinnedModel.sileroVAD.repository)
  ) {
    super.init(location: location) { installed, directory, report in
      let model = try await PinnedDownload.loadCompiled(
        .sileroVAD, compiled: Self.compiledModel, in: directory, installed: installed,
        report: report)
      try SileroVoiceActivity.checkFrameSize(of: model)
      return model
    }
  }
}

/// One stream of microphone audio read by the model. The state of the model's recurrent layer and
/// the 64 samples of context before each frame belong to the stream, so two streams never share
/// them; the lock keeps a call from another thread from corrupting its own.
final class SileroVoiceActivity: VoiceActivityDetecting, @unchecked Sendable {
  /// The samples the model reads before each frame, taken from the end of the frame before.
  static let contextSamples = 64
  /// The model's own frame, from the pinned `config.json` (`chunk_size`).
  static let chunkSize = 512
  private static let stateShape: [NSNumber] = [1, 1, 128]

  private let lock = NSLock()
  private let model: MLModel
  private var context = [Float](repeating: 0, count: contextSamples)
  private var hidden: MLMultiArray?
  private var cell: MLMultiArray?

  init(_ model: MLModel) { self.model = model }

  /// The compiled model reads `contextSamples + chunkSize` samples; a model that reads another
  /// frame is not this one.
  static func checkFrameSize(of model: MLModel) throws {
    let shape = model.modelDescription.inputDescriptionsByName["audio"]?.multiArrayConstraint?.shape
    let samples = shape?.last?.intValue
    guard samples == contextSamples + chunkSize else {
      throw VoiceModelError.frameSize(expected: contextSamples + chunkSize, got: samples ?? 0)
    }
  }

  func probability(of frame: [Float]) throws -> Double {
    guard frame.count == Self.chunkSize else {
      throw VoiceModelError.frameSize(expected: Self.chunkSize, got: frame.count)
    }
    return try lock.withLock {
      do {
        return try predict(context + frame, then: frame)
      } catch {
        throw VoiceModelError.prediction(error.localizedDescription)
      }
    }
  }

  /// A new stream starts clean: nothing of an earlier failure is remembered here (the loop decides
  /// whether to trust the model again, and it does at every start).
  func reset() {
    lock.withLock {
      context = [Float](repeating: 0, count: Self.contextSamples)
      hidden = nil
      cell = nil
    }
  }

  private func predict(_ samples: [Float], then frame: [Float]) throws -> Double {
    let audio = try MLMultiArray(shape: [1, 1, NSNumber(value: samples.count)], dataType: .float16)
    let values = audio.dataPointer.assumingMemoryBound(to: Float16.self)
    for (index, sample) in samples.enumerated() { values[index] = Float16(sample) }
    let h = try hidden ?? Self.zeros()
    let c = try cell ?? Self.zeros()
    let output = try model.prediction(
      from: MLDictionaryFeatureProvider(dictionary: [
        "audio": MLFeatureValue(multiArray: audio), "h": MLFeatureValue(multiArray: h),
        "c": MLFeatureValue(multiArray: c),
      ]))
    guard let nextH = output.featureValue(for: "h_out")?.multiArrayValue,
      let nextC = output.featureValue(for: "c_out")?.multiArrayValue,
      let probability = output.featureValue(for: "probability")?.multiArrayValue
    else { throw VoiceModelError.prediction("the model returned no probability") }
    let speech = try Self.value(of: probability)
    guard (0...1).contains(speech) else {
      throw VoiceModelError.prediction("the model returned a probability outside 0 to 1")
    }
    hidden = nextH
    cell = nextC
    context = Array(frame.suffix(Self.contextSamples))
    return speech
  }

  /// The probability, whichever of the two float types the model writes it in; another type is an
  /// error, not a number read from the wrong bytes.
  static func value(of array: MLMultiArray) throws -> Double {
    guard array.count >= 1 else { throw VoiceModelError.prediction("the model returned no value") }
    switch array.dataType {
    case .float16: return Double(array.dataPointer.assumingMemoryBound(to: Float16.self)[0])
    case .float32: return Double(array.dataPointer.assumingMemoryBound(to: Float.self)[0])
    default: throw VoiceModelError.prediction("the model returned a type this client cannot read")
    }
  }

  private static func zeros() throws -> MLMultiArray {
    let array = try MLMultiArray(shape: stateShape, dataType: .float16)
    let values = array.dataPointer.assumingMemoryBound(to: Float16.self)
    for index in 0..<array.count { values[index] = 0 }
    return array
  }
}
