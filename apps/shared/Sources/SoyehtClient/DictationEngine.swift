import Foundation

#if canImport(UIKit)
  import UIKit
#endif

/// The devices dictation runs on. An engine lists the ones it supports; the Preferences screen
/// shows the others as unavailable instead of hiding them.
public enum DictationPlatform: String, CaseIterable, Sendable {
  case mac
  case iPad
  case iPhone

  /// The device this process runs on.
  @MainActor public static var current: DictationPlatform {
    #if canImport(UIKit)
      UIDevice.current.userInterfaceIdiom == .pad ? .iPad : .iPhone
    #else
      .mac
    #endif
  }

  public var title: String {
    switch self {
    case .mac: "Mac"
    case .iPad: "iPad"
    case .iPhone: "iPhone"
    }
  }
}

/// A language an engine can transcribe: a BCP-47 tag, or one of the two selections every
/// engine understands.
public struct DictationLanguage: Identifiable, Hashable, Sendable {
  /// Follow the language of the device when the engine supports it.
  public static let system = DictationLanguage(id: "system", title: "System default")
  /// The engine detects the spoken language itself; only engines that can offer it.
  public static let automatic = DictationLanguage(id: "auto", title: "Detect automatically")

  public let id: String
  public let title: String

  public init(id: String, title: String) {
    self.id = id
    self.title = title
  }

  public init(tag: String) {
    self.init(id: tag, title: Locale.current.localizedString(forIdentifier: tag) ?? tag)
  }
}

public enum DictationAvailability: Equatable, Sendable {
  case available
  /// Why the engine cannot run on this device; shown next to its name in Preferences.
  case unavailable(String)
}

/// One speech engine the person can choose. The catalogue (`DictationEngineRegistry`) is the
/// only list of engines: everything else, Preferences, the badge, the shortcut and the
/// composer, is generated from these descriptors.
public struct DictationEngine: Identifiable, Sendable {
  public let id: String
  /// Shown in Preferences.
  public let name: String
  /// Short name shown above the mic.
  public let badge: String
  public let summary: String
  public let platforms: Set<DictationPlatform>
  /// Languages the engine can transcribe, `.system` first.
  public let languages: [DictationLanguage]
  /// Whether the person's vocabulary biases this engine.
  public let usesVocabulary: Bool
  /// Set when the engine needs a model installed before it can listen.
  public let model: DictationModelInfo?
  /// Runtime check on top of `platforms`, for example an OS version or a chip.
  public let availability: @Sendable () -> DictationAvailability
  public let make: @MainActor @Sendable () -> SpeechRecognizing

  public init(
    id: String, name: String, badge: String, summary: String,
    platforms: Set<DictationPlatform>, languages: [DictationLanguage],
    usesVocabulary: Bool = false, model: DictationModelInfo? = nil,
    availability: @escaping @Sendable () -> DictationAvailability = { .available },
    make: @escaping @MainActor @Sendable () -> SpeechRecognizing
  ) {
    self.id = id
    self.name = name
    self.badge = badge
    self.summary = summary
    self.platforms = platforms
    self.languages = languages
    self.usesVocabulary = usesVocabulary
    self.model = model
    self.availability = availability
    self.make = make
  }

  /// Why this engine cannot run on `platform`, or nil when it can.
  public func unavailableReason(on platform: DictationPlatform) -> String? {
    guard platforms.contains(platform) else { return "Not available on \(platform.title)" }
    if case .unavailable(let reason) = availability() { return reason }
    return nil
  }

  /// The language tag to give the engine for a selection, or nil to let the engine choose
  /// (its own default, or automatic detection). "System default" only becomes a tag when the
  /// engine ships that exact language: a Mac set to English in Brazil reports `en_BR`, which
  /// no engine lists, and guessing `en` would transcribe Portuguese speech as English.
  public func languageTag(
    for selection: String, systemIdentifier: String = Locale.current.identifier
  ) -> String? {
    switch selection {
    case DictationLanguage.automatic.id: return nil
    case DictationLanguage.system.id:
      let tag = systemIdentifier.replacingOccurrences(of: "_", with: "-")
      return languages.contains { $0.id == tag } ? tag : nil
    default: return selection
    }
  }
}

/// The catalogue of dictation engines, and the single place an engine is added.
///
/// The first engine is the fallback: it needs no download and runs everywhere the composer
/// does, so dictation always works while another engine downloads or when it is unavailable.
public struct DictationEngineRegistry: Sendable {
  public let engines: [DictationEngine]

  /// Engines with an id already in the list are ignored, so the first entry wins.
  public init(fallback: DictationEngine, others: [DictationEngine] = []) {
    var seen: Set<String> = [fallback.id]
    engines = [fallback] + others.filter { seen.insert($0.id).inserted }
  }

  public var fallback: DictationEngine { engines[0] }

  public func engine(id: String) -> DictationEngine? {
    engines.first { $0.id == id }
  }

  /// Apple's engine alone: what the shared package ships before an app adds more.
  public static let appleOnly = DictationEngineRegistry(fallback: .apple)
}
