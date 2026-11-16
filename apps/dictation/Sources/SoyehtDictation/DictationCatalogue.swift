import SoyehtClient

/// The dictation engines of the app, and the one place a new engine is added: write its file in
/// this module and list it here. Preferences, the shortcut, the mic's badge and the composer are
/// generated from this list.
///
/// Cloud engines are the person's own configurations, so the app passes them in:
/// `DictationEngine.cloud(_:vault:)` builds each descriptor.
public enum DictationCatalogue {
  public static let registry = registry(adding: [])

  public static func registry(adding extra: [DictationEngine]) -> DictationEngineRegistry {
    DictationEngineRegistry(fallback: .apple, others: [NemotronEngine.engine] + extra)
  }
}
