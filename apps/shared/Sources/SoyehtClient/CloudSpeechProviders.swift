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

extension CloudSpeechConfiguration {
  /// Available once the configuration is usable and, when the provider wants one, its key is in
  /// the vault. Reads the vault each time it is asked, so a key saved a moment ago counts.
  fileprivate func availability(_ vault: any ProviderKeyVault) -> DictationAvailability {
    if let problem { return .unavailable(problem) }
    guard requiresKey else { return .available }
    do {
      return try vault.key(for: id) == nil
        ? .unavailable(CloudSpeechError.missingKey.localizedDescription) : .available
    } catch {
      return .unavailable(error.localizedDescription)
    }
  }
}

extension SpeechSynthesisProvider {
  /// The provider of a cloud configuration, for the voice screen's catalogue. The text to speak
  /// is what it sends.
  public static func cloud(
    _ configuration: CloudSpeechConfiguration, vault: any ProviderKeyVault,
    transport: any SpeechHTTPTransport = URLSessionSpeechTransport()
  ) -> SpeechSynthesisProvider {
    let requests = CloudSpeechRequests(
      configuration: configuration, vault: vault, transport: transport)
    return SpeechSynthesisProvider(
      id: configuration.id, name: configuration.name,
      summary: "Speaks through \(configuration.host). Your text is sent there.",
      platforms: Set(DictationPlatform.allCases),
      voices: {
        configuration.voices.map { SpeechVoice(id: $0, name: $0, language: "und") }
      },
      cloud: CloudProviderInfo(
        host: configuration.host, sends: [.text], requiresKey: configuration.requiresKey),
      availability: { configuration.availability(vault) },
      make: { CloudSpeechSynthesizer(requests: requests) })
  }
}

extension DictationEngine {
  /// The dictation engine of a cloud configuration. The recording is what it sends, and the
  /// person's vocabulary goes along as the prompt that biases the transcript.
  public static func cloud(
    _ configuration: CloudSpeechConfiguration, vault: any ProviderKeyVault,
    transport: any SpeechHTTPTransport = URLSessionSpeechTransport()
  ) -> DictationEngine {
    let requests = CloudSpeechRequests(
      configuration: configuration, vault: vault, transport: transport)
    return DictationEngine(
      id: configuration.id, name: configuration.name, badge: configuration.name,
      summary: "Transcribes through \(configuration.host). Your recording is sent there.",
      platforms: Set(DictationPlatform.allCases),
      languages: [.system, .automatic] + configuration.languages.map(DictationLanguage.init(tag:)),
      usesVocabulary: true,
      cloud: CloudProviderInfo(
        host: configuration.host, sends: [.audio], requiresKey: configuration.requiresKey),
      availability: { configuration.availability(vault) },
      make: { CloudSpeechRecognizer(requests: requests) })
  }
}
