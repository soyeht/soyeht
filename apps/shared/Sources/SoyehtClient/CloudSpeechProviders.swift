import Foundation

/// Text to speech through a cloud provider's `/audio/speech` endpoint.
struct CloudSpeechSynthesizer: SpeechSynthesizing {
  let requests: CloudSpeechRequests

  func synthesize(_ text: String, options: SpeechSynthesisOptions) async throws
    -> SynthesizedSpeech
  {
    try await requests.speak(try SpeechSynthesisError.utterance(text), options: options)
  }
}

extension SpeechSynthesisProvider {
  /// The provider of a cloud configuration, for the voice screen's catalogue, or nil when the
  /// provider has no model that speaks. The text to speak is what it sends. Its license is the
  /// provider's own terms, which the configuration links.
  public static func cloud(
    _ configuration: CloudSpeechConfiguration, vault: any ProviderKeyVault,
    transport: any SpeechHTTPTransport = URLSessionSpeechTransport()
  ) -> SpeechSynthesisProvider? {
    guard configuration.speaks else { return nil }
    let requests = CloudSpeechRequests(
      configuration: configuration, vault: vault, transport: transport)
    return SpeechSynthesisProvider(
      id: configuration.id, name: configuration.name,
      summary: "Speaks through \(configuration.host). Your text is sent there.",
      platforms: Set(DictationPlatform.allCases),
      license: ProviderLicense(
        name: "The provider's terms", url: configuration.termsURL, commercialUse: true),
      voices: {
        configuration.voices.map { SpeechVoice(id: $0, name: $0, language: "und") }
      },
      cloud: CloudProviderInfo(
        host: configuration.host, sends: [.text], termsURL: configuration.termsURL,
        requiresKey: configuration.requiresKey),
      availability: { configuration.availability(vault) },
      make: { CloudSpeechSynthesizer(requests: requests) })
  }
}

extension DictationEngine {
  /// The dictation engine of a cloud configuration, or nil when the provider has no model that
  /// transcribes. What it sends is the recording and the person's vocabulary, which goes along
  /// as the prompt that biases the transcript.
  public static func cloud(
    _ configuration: CloudSpeechConfiguration, vault: any ProviderKeyVault,
    transport: any SpeechHTTPTransport = URLSessionSpeechTransport()
  ) -> DictationEngine? {
    guard configuration.transcribes else { return nil }
    let requests = CloudSpeechRequests(
      configuration: configuration, vault: vault, transport: transport)
    return DictationEngine(
      id: configuration.id, name: configuration.name, badge: configuration.name,
      summary:
        "Transcribes through \(configuration.host). Your recording and your vocabulary are sent there.",
      platforms: Set(DictationPlatform.allCases),
      languages: [.system, .automatic] + configuration.languages.map(DictationLanguage.init(tag:)),
      usesVocabulary: true,
      cloud: CloudProviderInfo(
        host: configuration.host, sends: [.audio, .vocabulary], termsURL: configuration.termsURL,
        requiresKey: configuration.requiresKey),
      availability: { configuration.availability(vault) },
      make: { CloudSpeechRecognizer(requests: requests) })
  }
}
