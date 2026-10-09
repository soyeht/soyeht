import SoyehtClient

/// The dictation engines of the app, and the one place a new engine is added: write its file in
/// this module and list it here. Preferences, the shortcut, the mic's badge and the composer are
/// generated from this list.
public enum DictationCatalogue {
  public static let registry = DictationEngineRegistry(
    fallback: .apple,
    others: [
      NemotronEngine.engine
    ])
}
