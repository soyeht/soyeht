import Foundation

/// What a cloud provider receives, named before the first use so the person agrees to it.
public enum CloudPayload: String, CaseIterable, Sendable {
  /// The person's voice, recorded while they talk.
  case audio
  /// The words to speak or the words transcribed.
  case text
}

/// A provider that runs on someone else's computer. The configuration screen shows this before
/// the first use; the key lives in the vault of the device that calls the provider.
public struct CloudProviderInfo: Sendable {
  /// The host that receives the request, for example `api.example.com`.
  public let host: String
  public let sends: Set<CloudPayload>
  /// The provider's terms or privacy statement, when it has one the person can read.
  public let termsURL: URL?
  /// Whether the provider wants an API key before it answers.
  public let requiresKey: Bool

  public init(
    host: String, sends: Set<CloudPayload>, termsURL: URL? = nil, requiresKey: Bool = true
  ) {
    self.host = host
    self.sends = sends
    self.termsURL = termsURL
    self.requiresKey = requiresKey
  }
}

/// One text-to-speech provider the person can choose. The catalogue
/// (`SpeechSynthesisRegistry`) is the only list of them: the voice screen is generated from these
/// descriptors, and nothing else knows a provider by name.
public struct SpeechSynthesisProvider: Identifiable, Sendable {
  public let id: String
  /// Shown in the voice screen.
  public let name: String
  public let summary: String
  public let platforms: Set<DictationPlatform>
  /// The voices the provider speaks with, first the default one. Read when the screen opens.
  public let voices: @Sendable () -> [SpeechVoice]
  /// Set when the provider needs a model installed before it can speak; the install is
  /// `DictationModelStore`'s, the same as a dictation engine's.
  public let model: DictationModelInfo?
  /// Set when the provider is someone else's computer.
  public let cloud: CloudProviderInfo?
  /// Runtime check on top of `platforms`: an OS version, a chip, a missing key.
  public let availability: @Sendable () -> DictationAvailability
  public let make: @Sendable () -> any SpeechSynthesizing

  public init(
    id: String, name: String, summary: String, platforms: Set<DictationPlatform>,
    voices: @escaping @Sendable () -> [SpeechVoice], model: DictationModelInfo? = nil,
    cloud: CloudProviderInfo? = nil,
    availability: @escaping @Sendable () -> DictationAvailability = { .available },
    make: @escaping @Sendable () -> any SpeechSynthesizing
  ) {
    self.id = id
    self.name = name
    self.summary = summary
    self.platforms = platforms
    self.voices = voices
    self.model = model
    self.cloud = cloud
    self.availability = availability
    self.make = make
  }

  /// A provider requires an install exactly when it has a model.
  public var requiresInstall: Bool { model != nil }

  /// Why this provider cannot speak on `platform`, or nil when it can.
  public func unavailableReason(on platform: DictationPlatform) -> String? {
    availability().reason(on: platform, supportedBy: platforms)
  }
}

/// The catalogue of text-to-speech providers, and the single place one is added.
///
/// The first provider is the fallback: it needs no download, no key and no network, so the
/// avatar can always speak.
public struct SpeechSynthesisRegistry: Sendable {
  public let providers: [SpeechSynthesisProvider]

  /// Providers with an id already in the list are ignored, so the first entry wins.
  public init(fallback: SpeechSynthesisProvider, others: [SpeechSynthesisProvider] = []) {
    var seen: Set<String> = [fallback.id]
    providers = [fallback] + others.filter { seen.insert($0.id).inserted }
  }

  public var fallback: SpeechSynthesisProvider { providers[0] }

  public func provider(id: String) -> SpeechSynthesisProvider? {
    providers.first { $0.id == id }
  }
}
