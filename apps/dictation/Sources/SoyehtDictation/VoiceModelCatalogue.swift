import SoyehtClient

/// The models of the voice loop, and the one place one is added: write its file in this module and
/// list it here. Each catalogue starts with the entry that needs no download, which the loop uses
/// until the person installs and chooses another.
public enum VoiceModelCatalogue {
  public static let activity = VoiceActivityRegistry(
    fallback: .energy, others: [SileroEngine.provider])

  public static let endOfTurn = EndOfTurnRegistry(
    fallback: .silence, others: [SmartTurnEngine.provider])
}
