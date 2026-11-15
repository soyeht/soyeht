import Foundation

/// A cloud provider the person added: a base URL, the models it offers and a key, over the audio
/// endpoints that several providers share (`/audio/transcriptions` and `/audio/speech`). A
/// provider may offer only one of the two. The key is not part of it: it lives in a
/// `ProviderKeyVault` under `id`.
public struct CloudSpeechConfiguration: Codable, Equatable, Identifiable, Sendable {
  /// Common languages offered when the person does not choose their own.
  public static let defaultLanguages = [
    "en-US", "pt-BR", "es-ES", "fr-FR", "de-DE", "it-IT", "ja-JP", "ko-KR", "zh-CN",
  ]

  /// Names the provider in the catalogue and the vault, for example `cloud.my-provider`.
  public let id: String
  public var name: String
  /// The endpoint root, for example `https://api.example.com/v1`.
  public var baseURL: URL
  /// The model that transcribes, sent as `model` to `/audio/transcriptions`. Nil or empty when
  /// the provider does not transcribe: it then is not offered as a dictation engine.
  public var transcriptionModel: String?
  /// The model that speaks, sent as `model` to `/audio/speech`. Nil or empty when the provider
  /// does not speak: it then is not offered as a voice.
  public var speechModel: String?
  /// The voices of the provider, first the default one.
  public var voices: [String]
  /// The languages offered for transcription, as BCP-47 tags.
  public var languages: [String]
  /// False for a server that takes no key, on the person's own machine or network.
  public var requiresKey: Bool
  /// The provider's terms or privacy statement, shown with what the provider receives.
  public var termsURL: URL?

  public init(
    id: String, name: String, baseURL: URL, transcriptionModel: String? = nil,
    speechModel: String? = nil, voices: [String] = [],
    languages: [String] = Self.defaultLanguages, requiresKey: Bool = true, termsURL: URL? = nil
  ) {
    self.id = id
    self.name = name
    self.baseURL = baseURL
    self.transcriptionModel = transcriptionModel
    self.speechModel = speechModel
    self.voices = voices
    self.languages = languages
    self.requiresKey = requiresKey
    self.termsURL = termsURL
  }

  public var transcribes: Bool { !(transcriptionModel ?? "").isEmpty }
  public var speaks: Bool { !(speechModel ?? "").isEmpty }

  /// Why this configuration cannot be used, or nil. A key only travels over HTTPS or to this
  /// computer. Plain HTTP is for a server that takes no key (the person's own machine, or one on
  /// their own network) and for a loopback address.
  public var problem: String? {
    guard let scheme = baseURL.scheme?.lowercased(), let host = baseURL.host, !host.isEmpty else {
      return "The address must be a full URL such as https://api.example.com/v1."
    }
    switch scheme {
    case "https": break
    case "http" where !requiresKey || Self.isLoopback(host): break
    default:
      return "The address must use https, unless the server runs on this computer or takes no key."
    }
    if !transcribes && !speaks {
      return "Name a model the provider uses to transcribe or to speak."
    }
    return nil
  }

  var host: String { baseURL.host ?? "" }

  func endpoint(_ name: String) -> URL { baseURL.appendingPathComponent(name) }

  /// The key to send, or nil when the provider takes none. Throws why the provider cannot be
  /// used now: an address that is wrong, or a key that is not in the vault. This is the one place
  /// that decides it.
  func credential(in vault: any ProviderKeyVault) throws -> String? {
    if let problem { throw CloudSpeechError.invalidConfiguration(problem) }
    guard requiresKey else { return nil }
    guard let key = try vault.key(for: id), !key.isEmpty else { throw CloudSpeechError.missingKey }
    return key
  }

  /// `credential(in:)` as the availability a descriptor reports.
  func availability(_ vault: any ProviderKeyVault) -> DictationAvailability {
    do {
      _ = try credential(in: vault)
      return .available
    } catch {
      return .unavailable(error.localizedDescription)
    }
  }

  private static func isLoopback(_ host: String) -> Bool {
    ["localhost", "127.0.0.1", "::1"].contains(host.lowercased())
  }
}
