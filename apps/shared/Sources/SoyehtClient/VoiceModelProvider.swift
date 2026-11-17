import Foundation

/// One model of the voice loop the person can choose: a voice-activity model (what is speech) or an
/// end-of-turn model (whether the person has finished). Like a dictation engine or a voice, it is a
/// descriptor in a catalogue, and nothing else knows a model by name. A model is opt-in: it is
/// downloaded after the person presses Install, through `DictationModelStore`, and the entry that
/// needs no model is the fallback that works until then.
public struct VoiceModelProvider<Made: Sendable>: Identifiable, Sendable {
  public let id: String
  public let name: String
  public let summary: String
  public let platforms: Set<DictationPlatform>
  /// The license of the model and of the code that runs it.
  public let license: ProviderLicense
  /// Set when the model needs an install before it can run.
  public let model: DictationModelInfo?
  /// Runtime check on top of `platforms`: a chip, or the memory of the device.
  public let availability: @Sendable () -> DictationAvailability
  /// A detector for one stream, or nil while the model is not installed and loaded.
  public let make: @Sendable () -> Made?

  public init(
    id: String, name: String, summary: String, platforms: Set<DictationPlatform>,
    license: ProviderLicense, model: DictationModelInfo? = nil,
    availability: @escaping @Sendable () -> DictationAvailability = { .available },
    make: @escaping @Sendable () -> Made?
  ) {
    self.id = id
    self.name = name
    self.summary = summary
    self.platforms = platforms
    self.license = license
    self.model = model
    self.availability = availability
    self.make = make
  }

  public func unavailableReason(on platform: DictationPlatform) -> String? {
    availability().reason(on: platform, supportedBy: platforms)
  }
}

public typealias VoiceActivityProvider = VoiceModelProvider<any VoiceActivityDetecting>
public typealias EndOfTurnProvider = VoiceModelProvider<any EndOfTurnDetecting>
public typealias VoiceActivityRegistry = ProviderRegistry<VoiceActivityProvider>
public typealias EndOfTurnRegistry = ProviderRegistry<EndOfTurnProvider>

extension VoiceModelProvider where Made == any VoiceActivityDetecting {
  /// Loudness alone: no download, every device, the fallback of the catalogue.
  public static let energy = VoiceActivityProvider(
    id: "energy", name: "Loudness",
    summary: "Built in. Takes any loud sound for speech, so it suits a quiet room.",
    platforms: Set(DictationPlatform.allCases),
    license: ProviderLicense(name: "Part of Soyeht", commercialUse: true),
    make: { EnergyVoiceActivity() })
}

extension VoiceModelProvider where Made == any EndOfTurnDetecting {
  /// A pause is the end of a turn: no download, every device, the fallback of the catalogue.
  public static let silence = EndOfTurnProvider(
    id: "silence", name: "Pause",
    summary: "Built in. The turn ends after a short pause, even in the middle of a thought.",
    platforms: Set(DictationPlatform.allCases),
    license: ProviderLicense(name: "Part of Soyeht", commercialUse: true),
    make: { SilenceEndOfTurn() })
}
