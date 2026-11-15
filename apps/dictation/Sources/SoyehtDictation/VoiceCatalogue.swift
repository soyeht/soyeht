import SoyehtClient

/// The text-to-speech providers of the app, and the one place a provider is added: write its
/// file in this module (or in `apps/shared` when it has no third-party dependency) and list it
/// here. The voice screen is generated from this list.
///
/// Cloud providers are the person's own configurations, so the app passes them in:
/// `SpeechSynthesisProvider.cloud(_:vault:)` builds each descriptor.
public enum VoiceCatalogue {
  public static let synthesis = synthesis(adding: [])

  public static func synthesis(adding extra: [SpeechSynthesisProvider])
    -> SpeechSynthesisRegistry
  {
    SpeechSynthesisRegistry(fallback: .apple, others: [KokoroEngine.provider] + extra)
  }
}
