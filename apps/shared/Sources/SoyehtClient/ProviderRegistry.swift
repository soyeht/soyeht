import Foundation

/// A catalogue of providers of one kind, and the single place one is added.
///
/// The first entry is the fallback: it needs no download, no key and no network, so the feature
/// always works while another provider installs or when it is unavailable. Dictation engines and
/// text-to-speech providers are both one of these.
public struct ProviderRegistry<Entry: Identifiable & Sendable>: Sendable where Entry.ID == String {
  public let entries: [Entry]

  /// Entries with an id already in the list are ignored, so the first one wins.
  public init(fallback: Entry, others: [Entry] = []) {
    var seen: Set<String> = [fallback.id]
    entries = [fallback] + others.filter { seen.insert($0.id).inserted }
  }

  public var fallback: Entry { entries[0] }

  public func entry(id: String) -> Entry? {
    entries.first { $0.id == id }
  }
}

/// The catalogue of dictation engines.
public typealias DictationEngineRegistry = ProviderRegistry<DictationEngine>

extension ProviderRegistry where Entry == DictationEngine {
  public var engines: [DictationEngine] { entries }

  public func engine(id: String) -> DictationEngine? { entry(id: id) }

  /// Apple's engine alone: what the shared package ships before an app adds more.
  public static var appleOnly: Self { Self(fallback: .apple) }
}

/// The catalogue of text-to-speech providers.
public typealias SpeechSynthesisRegistry = ProviderRegistry<SpeechSynthesisProvider>

extension ProviderRegistry where Entry == SpeechSynthesisProvider {
  public var providers: [SpeechSynthesisProvider] { entries }

  public func provider(id: String) -> SpeechSynthesisProvider? { entry(id: id) }
}
