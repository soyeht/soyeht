import Foundation
import SoyehtClient

/// NVIDIA Nemotron 3.5 ASR Streaming (600M parameters, cache-aware FastConformer with an RNN-T
/// decoder), as the INT8 CoreML bundle. It is opt-in: it runs on this device after a one-time
/// download that the person starts from Preferences. It adds automatic language detection and
/// uses the person's vocabulary.
public enum NemotronEngine {
  /// The transcription-ready languages of the model.
  private static let languageTags = [
    "pt-BR", "pt-PT", "en-US", "en-GB", "es-ES", "es-US", "fr-FR", "fr-CA", "de-DE", "it-IT",
    "nl-NL", "tr-TR", "ru-RU", "ar-AR", "hi-IN", "ja-JP", "ko-KR", "vi-VN", "uk-UA",
  ]

  public static let engine = DictationEngine(
    id: "nemotron", name: "NVIDIA Nemotron 3.5", badge: "Nemotron",
    summary:
      "Streams on this device in 19 languages, detects the language by itself and learns your vocabulary.",
    platforms: Set(DictationPlatform.allCases),
    languages: [.system, .automatic] + languageTags.map(DictationLanguage.init(tag:)),
    usesVocabulary: true,
    model: DictationModelInfo(
      sizeDescription: "about 642 MB", licenseName: "OpenMDW-1.1",
      licenseURL: URL(string: "https://openmdw.ai/license/1-1/"),
      install: { report in try await NemotronModelHolder.shared.install(report: report) },
      isInstalled: { NemotronModelHolder.shared.isInstalled },
      remove: { try NemotronModelHolder.shared.remove() }),
    availability: Self.availability,
    make: { NemotronRecognizer() })

  /// The CoreML bundle needs an arm64 chip. On iPhone and iPad the weights and the compiled
  /// models take more than a gigabyte of memory, so devices below 6 GB of RAM keep Apple's
  /// engine.
  @Sendable static func availability() -> DictationAvailability {
    #if arch(arm64)
      #if os(iOS)
        guard ProcessInfo.processInfo.physicalMemory >= minimumMemory else {
          return .unavailable("Needs a device with at least 6 GB of memory")
        }
      #endif
      return .available
    #else
      return .unavailable("Needs Apple silicon")
    #endif
  }

  #if os(iOS)
    private static let minimumMemory: UInt64 = 6 << 30
  #endif
}
