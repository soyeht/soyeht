import Foundation
import Testing

@testable import SoyehtClient

private let secret = "sk-test-0123456789"

private func requests(
  _ configuration: CloudSpeechConfiguration = cloudConfiguration(),
  vault: MemoryProviderKeyVault = MemoryProviderKeyVault(["cloud.example": secret]),
  transport: FakeSpeechTransport
) -> CloudSpeechRequests {
  CloudSpeechRequests(configuration: configuration, vault: vault, transport: transport)
}

private func jsonBody(of request: URLRequest) throws -> [String: Any] {
  let body = try #require(request.httpBody)
  return try #require(try JSONSerialization.jsonObject(with: body) as? [String: Any])
}

private func text(_ data: Data?) -> String { String(decoding: data ?? Data(), as: UTF8.self) }

// MARK: Configuration

@Test func aKeyOnlyTravelsOverHTTPSOrToThisComputer() {
  #expect(cloudConfiguration().problem == nil)
  #expect(cloudConfiguration(base: "http://localhost:8880/v1").problem == nil)
  #expect(cloudConfiguration(base: "http://127.0.0.1:8880/v1").problem == nil)
  // A server that wants a key is only reached over HTTPS, or on this computer.
  #expect(cloudConfiguration(base: "http://speech.example.com/v1").problem != nil)
  #expect(cloudConfiguration(base: "http://192.168.1.20:8880/v1").problem != nil)
  #expect(cloudConfiguration(base: "ftp://speech.example.com").problem != nil)
  #expect(cloudConfiguration(base: "not a url").problem != nil)
}

@Test func aServerThatTakesNoKeyMayBeReachedOverPlainHTTPOnTheOwnNetwork() {
  #expect(
    cloudConfiguration(requiresKey: false, base: "http://192.168.1.20:8880/v1").problem == nil)
  #expect(
    cloudConfiguration(requiresKey: false, base: "http://my-machine.tailnet.example:8880/v1")
      .problem == nil)
  #expect(cloudConfiguration(requiresKey: false, base: "ftp://192.168.1.20").problem != nil)
}

@Test func aProviderNeedsAModelForAtLeastOneOfTheTwoThings() {
  var configuration = cloudConfiguration()
  configuration.transcriptionModel = nil
  #expect(configuration.problem == nil)
  #expect(!configuration.transcribes && configuration.speaks)
  configuration.speechModel = ""
  #expect(configuration.problem == "Name a model the provider uses to transcribe or to speak.")
}

@Test func aProviderThatOnlyOneOfTheTwoIsOnlyOfferedAsThatOne() throws {
  let vault = MemoryProviderKeyVault()
  var speakOnly = cloudConfiguration()
  speakOnly.transcriptionModel = nil
  #expect(DictationEngine.cloud(speakOnly, vault: vault) == nil)
  #expect(SpeechSynthesisProvider.cloud(speakOnly, vault: vault) != nil)
  var listenOnly = cloudConfiguration()
  listenOnly.speechModel = nil
  #expect(SpeechSynthesisProvider.cloud(listenOnly, vault: vault) == nil)
  #expect(DictationEngine.cloud(listenOnly, vault: vault) != nil)
}

@Test func aRequestForWhatTheProviderDoesNotOfferSendsNothing() async {
  let transport = FakeSpeechTransport { _ in (200, Data()) }
  var listenOnly = cloudConfiguration()
  listenOnly.speechModel = nil
  await #expect(throws: CloudSpeechError.notOffered) {
    try await CloudSpeechSynthesizer(requests: requests(listenOnly, transport: transport))
      .synthesize("Hi", options: SpeechSynthesisOptions())
  }
  #expect(transport.requests.isEmpty)
}

@Test func anEmptyKeyIsAMissingKey() async {
  let transport = FakeSpeechTransport { _ in (200, Data()) }
  await #expect(throws: CloudSpeechError.missingKey) {
    try await requests(vault: MemoryProviderKeyVault(["cloud.example": ""]), transport: transport)
      .transcribe([0], language: nil, vocabulary: [])
  }
  #expect(transport.requests.isEmpty)
}

@Test func aRedirectIsReportedAndNeverFollowed() async {
  let transport = FakeSpeechTransport { _ in (302, Data()) }
  await #expect(throws: CloudSpeechError.redirected) {
    try await requests(transport: transport).transcribe([0], language: nil, vocabulary: [])
  }
  #expect(CloudSpeechError.redirected.localizedDescription.contains("another one"))
}

@Test func aConfigurationSurvivesTheStoreItIsSavedIn() throws {
  let configuration = cloudConfiguration()
  let decoded = try JSONDecoder().decode(
    CloudSpeechConfiguration.self, from: JSONEncoder().encode(configuration))
  #expect(decoded == configuration)
}

// MARK: Transcription

@Test func aRecordingIsSentAsAWaveFileWithTheKeyTheModelTheLanguageAndTheVocabulary() async throws {
  let transport = FakeSpeechTransport { _ in (200, Data(#"{"text":" hello there \n"}"#.utf8)) }
  let transcript = try await requests(transport: transport).transcribe(
    [0, 0.5, -0.5], language: "pt-BR", vocabulary: ["Soyeht", "ptyd"])
  #expect(transcript == "hello there")

  let sent = try #require(transport.requests.first)
  #expect(sent.url?.absoluteString == "https://speech.example.com/v1/audio/transcriptions")
  #expect(sent.httpMethod == "POST")
  #expect(sent.value(forHTTPHeaderField: "Authorization") == "Bearer \(secret)")
  #expect(
    sent.value(forHTTPHeaderField: "Content-Type")?.hasPrefix("multipart/form-data; boundary=")
      == true)
  let body = text(sent.httpBody)
  #expect(body.contains("name=\"model\"\r\n\r\nlisten-1"))
  #expect(body.contains("name=\"language\"\r\n\r\npt"))
  #expect(body.contains("name=\"prompt\"\r\n\r\nSoyeht, ptyd"))
  #expect(body.contains("name=\"file\"; filename=\"audio.wav\""))
  #expect(body.contains("RIFF"))
  #expect(body.hasSuffix("--\r\n"))
}

@Test func noLanguageAndNoVocabularyLeaveTheirFieldsOut() async throws {
  let transport = FakeSpeechTransport { _ in (200, Data(#"{"text":"ok"}"#.utf8)) }
  _ = try await requests(transport: transport).transcribe([0], language: nil, vocabulary: [])
  let body = text(transport.requests.first?.httpBody)
  #expect(!body.contains("name=\"language\""))
  #expect(!body.contains("name=\"prompt\""))
}

@Test func theVocabularyPromptStopsBeforeItGetsTooLong() {
  let phrases = (0..<200).map { "phrase-\($0)" }
  let prompt = CloudSpeechRequests.prompt(from: phrases)
  #expect(prompt.count <= CloudSpeechRequests.maximumPromptCharacters)
  #expect(prompt.hasPrefix("phrase-0, phrase-1"))
  #expect(!prompt.hasSuffix(","))
}

@Test func theLanguageTagBecomesTheCodeTheEndpointsTake() {
  #expect(CloudSpeechRequests.primaryLanguage("pt-BR") == "pt")
  #expect(CloudSpeechRequests.primaryLanguage("en_US") == "en")
  #expect(CloudSpeechRequests.primaryLanguage("ja") == "ja")
}

// MARK: Speech

@Test func textIsSentAsJSONAndTheWaveFileThatComesBackIsRead() async throws {
  let reply = WaveAudio.encode([0.25, -0.25, 0.5], sampleRate: 24_000)
  let transport = FakeSpeechTransport { _ in (200, reply) }
  let speech = try await CloudSpeechSynthesizer(requests: requests(transport: transport))
    .synthesize("  Good morning.  ", options: SpeechSynthesisOptions(voice: "stone", speed: 1.25))
  #expect(speech.sampleRate == 24_000)
  #expect(speech.samples.count == 3)

  let sent = try #require(transport.requests.first)
  #expect(sent.url?.absoluteString == "https://speech.example.com/v1/audio/speech")
  #expect(sent.value(forHTTPHeaderField: "Authorization") == "Bearer \(secret)")
  let json = try jsonBody(of: sent)
  #expect(json["model"] as? String == "speak-1")
  #expect(json["input"] as? String == "Good morning.")
  #expect(json["voice"] as? String == "stone")
  #expect(json["response_format"] as? String == "wav")
  #expect(json["speed"] as? Double == 1.25)
}

@Test func theFirstVoiceOfTheConfigurationIsTheDefault() async throws {
  let transport = FakeSpeechTransport { _ in (200, WaveAudio.encode([0.1], sampleRate: 24_000)) }
  _ = try await CloudSpeechSynthesizer(requests: requests(transport: transport))
    .synthesize("Hi", options: SpeechSynthesisOptions())
  let json = try jsonBody(of: try #require(transport.requests.first))
  #expect(json["voice"] as? String == "river")
}

@Test func emptyTextIsRefusedBeforeAnythingIsSent() async {
  let transport = FakeSpeechTransport { _ in (200, Data()) }
  await #expect(throws: SpeechSynthesisError.emptyText) {
    try await CloudSpeechSynthesizer(requests: requests(transport: transport))
      .synthesize(" \n ", options: SpeechSynthesisOptions())
  }
  #expect(transport.requests.isEmpty)
}

@Test func anAnswerThatIsNotAWaveFileIsRefusedWithAReason() async {
  let transport = FakeSpeechTransport { _ in (200, Data("ID3 mp3 bytes".utf8)) }
  await #expect(throws: SpeechSynthesisError.unsupportedAudio("not a WAV file")) {
    try await CloudSpeechSynthesizer(requests: requests(transport: transport))
      .synthesize("Hi", options: SpeechSynthesisOptions())
  }
}

// MARK: Keys and failures

@Test func noKeyMeansNothingIsSent() async {
  let transport = FakeSpeechTransport { _ in (200, Data()) }
  let empty = MemoryProviderKeyVault()
  await #expect(throws: CloudSpeechError.missingKey) {
    try await requests(vault: empty, transport: transport)
      .transcribe([0], language: nil, vocabulary: [])
  }
  await #expect(throws: CloudSpeechError.missingKey) {
    try await CloudSpeechSynthesizer(requests: requests(vault: empty, transport: transport))
      .synthesize("Hi", options: SpeechSynthesisOptions())
  }
  #expect(transport.requests.isEmpty)
}

@Test func aServerThatTakesNoKeyIsCalledWithoutOne() async throws {
  let transport = FakeSpeechTransport { _ in (200, Data(#"{"text":"ok"}"#.utf8)) }
  let vault = MemoryProviderKeyVault()
  let local = cloudConfiguration(requiresKey: false, base: "http://localhost:8880/v1")
  _ = try await requests(local, vault: vault, transport: transport)
    .transcribe([0], language: nil, vocabulary: [])
  #expect(transport.requests.first?.value(forHTTPHeaderField: "Authorization") == nil)
  #expect(vault.reads == 0)
}

@Test func aConfigurationThatCannotBeUsedSendsNothing() async {
  let transport = FakeSpeechTransport { _ in (200, Data()) }
  let insecure = cloudConfiguration(base: "http://speech.example.com/v1")
  await #expect(throws: CloudSpeechError.self) {
    try await requests(insecure, transport: transport)
      .transcribe([0], language: nil, vocabulary: [])
  }
  #expect(transport.requests.isEmpty)
}

@Test func aRefusalSaysWhatHappenedAndNeverQuotesTheProvider() async {
  let body = Data(#"{"error":{"message":"Incorrect API key provided: sk-test-0123****6789"}}"#.utf8)
  for (status, expected) in [
    (401, "The provider did not accept the key."),
    (403, "The provider did not accept the key."),
    (429, "The provider is limiting requests. Try again in a moment."),
    (500, "The provider answered with an error (HTTP 500)."),
  ] {
    let transport = FakeSpeechTransport { _ in (status, body) }
    do {
      _ = try await requests(transport: transport).transcribe([0], language: nil, vocabulary: [])
      Issue.record("status \(status) should fail")
    } catch {
      #expect(error.localizedDescription == expected)
      #expect(!error.localizedDescription.contains("sk-test"))
    }
  }
}

@Test func anAnswerThatIsNotJSONIsTheFixedMessage() async {
  let transport = FakeSpeechTransport { _ in (200, Data("<html>sk-test-0123456789</html>".utf8)) }
  do {
    _ = try await requests(transport: transport).transcribe([0], language: nil, vocabulary: [])
    Issue.record("a page that is not JSON should fail")
  } catch {
    #expect(error as? CloudSpeechError == .badResponse)
    #expect(!error.localizedDescription.contains("sk-test"))
  }
}

// MARK: Descriptors

@MainActor @Test func theCloudProvidersTellWhatTheyReceiveAndWaitForAKey() throws {
  let vault = MemoryProviderKeyVault()
  let transport = FakeSpeechTransport { _ in (200, Data()) }
  let configuration = cloudConfiguration(termsURL: URL(string: "https://speech.example.com/terms"))
  let speech = try #require(
    SpeechSynthesisProvider.cloud(configuration, vault: vault, transport: transport))
  let dictation = try #require(
    DictationEngine.cloud(configuration, vault: vault, transport: transport))

  #expect(speech.cloud?.host == "speech.example.com")
  #expect(speech.cloud?.sends == [.text])
  // The recording, and the vocabulary that goes along as the prompt.
  #expect(dictation.cloud?.sends == [.audio, .vocabulary])
  #expect(dictation.summary.contains("vocabulary"))
  #expect(speech.cloud?.termsURL?.absoluteString == "https://speech.example.com/terms")
  #expect(dictation.cloud?.termsURL == speech.cloud?.termsURL)
  #expect(speech.license.url == speech.cloud?.termsURL)
  // The provider's terms are not known, so nothing is said about commercial use.
  #expect(speech.license.commercialUse == nil)
  #expect(!(speech.cloud?.sentUnencrypted ?? true))
  #expect(speech.requiresInstall == false)
  #expect(speech.voices().map(\.id) == ["river", "stone"])
  #expect(dictation.usesVocabulary)
  #expect(dictation.languages.prefix(2).map(\.id) == ["system", "auto"])
  #expect(dictation.languages.map(\.id).contains("pt-BR"))

  #expect(speech.unavailableReason(on: .mac) == "Add the provider's API key first.")
  #expect(dictation.unavailableReason(on: .mac) == "Add the provider's API key first.")
  try vault.save(secret, for: "cloud.example")
  #expect(speech.unavailableReason(on: .mac) == nil)
  #expect(dictation.unavailableReason(on: .mac) == nil)
  #expect(transport.requests.isEmpty)
}

@Test func noDescriptorTextCarriesTheKey() throws {
  let vault = MemoryProviderKeyVault(["cloud.example": secret])
  let speech = try #require(SpeechSynthesisProvider.cloud(cloudConfiguration(), vault: vault))
  for field in [speech.id, speech.name, speech.summary, speech.cloud?.host ?? ""] {
    #expect(!field.contains(secret))
  }
}

@Test func aCloudProviderWithABadAddressIsUnavailableWithTheReason() throws {
  let provider = try #require(
    SpeechSynthesisProvider.cloud(
      cloudConfiguration(base: "http://speech.example.com/v1"), vault: MemoryProviderKeyVault()))
  #expect(provider.unavailableReason(on: .mac)?.contains("https") == true)
}

@Test func aSpeakingProviderWithNoVoiceIsAProblemUpFront() {
  var configuration = cloudConfiguration()
  configuration.voices = []
  #expect(
    configuration.problem == "Name at least one voice for speech, or leave the speech model empty.")
  #expect(
    SpeechSynthesisProvider.cloud(
      configuration, vault: MemoryProviderKeyVault(["cloud.example": "k"]))?
      .unavailableReason(on: .mac) == configuration.problem)
  // Without the speech model the voices are not needed.
  configuration.speechModel = nil
  #expect(configuration.problem == nil)
}

@Test func aServerReachedOverPlainHTTPAcrossTheNetworkIsMarkedAsSendingInClear() throws {
  let vault = MemoryProviderKeyVault()
  let lan = cloudConfiguration(requiresKey: false, base: "http://192.168.1.20:8880/v1")
  #expect(lan.sentUnencrypted)
  let speech = try #require(SpeechSynthesisProvider.cloud(lan, vault: vault))
  let dictation = try #require(DictationEngine.cloud(lan, vault: vault))
  #expect(speech.cloud?.sentUnencrypted == true)
  #expect(dictation.cloud?.sentUnencrypted == true)
  // On this computer nothing leaves it, and HTTPS is encrypted.
  #expect(!cloudConfiguration(requiresKey: false, base: "http://localhost:8880/v1").sentUnencrypted)
  #expect(!cloudConfiguration().sentUnencrypted)
}
