import Foundation

/// What went wrong with a provider, without the provider's own text: an error body can echo part
/// of the key or the audio that was sent.
public enum CloudSpeechError: Error, LocalizedError, Equatable, Sendable {
  case invalidConfiguration(String)
  case missingKey
  case rejected(Int)
  /// The provider answered with a redirect, which is never followed.
  case redirected
  case responseTooLarge
  case badResponse
  /// The provider does not offer what was asked: it has no model for it.
  case notOffered

  public var errorDescription: String? {
    switch self {
    case .invalidConfiguration(let problem): problem
    case .missingKey: "Add the provider's API key first."
    case .rejected(401), .rejected(403): "The provider did not accept the key."
    case .rejected(429): "The provider is limiting requests. Try again in a moment."
    case .rejected(let status): "The provider answered with an error (HTTP \(status))."
    case .redirected:
      "The provider's address redirects to another one. Enter the address the provider gives for its API; a recording, a text or a key is never sent to a different one."
    case .responseTooLarge: "The provider's answer is too large."
    case .badResponse: "The provider's answer could not be read."
    case .notOffered: "This provider does not offer that."
    }
  }
}

/// The requests of the two endpoints, and the reading of their answers.
struct CloudSpeechRequests: Sendable {
  let configuration: CloudSpeechConfiguration
  let vault: any ProviderKeyVault
  let transport: any SpeechHTTPTransport

  /// A Whisper-class prompt is a few hundred tokens at most.
  static let maximumPromptCharacters = 600

  func transcribe(
    _ samples: [Float], language: String?, vocabulary: [String]
  ) async throws -> String {
    guard let model = configuration.transcriptionModel, configuration.transcribes else {
      throw CloudSpeechError.notOffered
    }
    var form = MultipartForm()
    form.add(name: "model", value: model)
    form.add(name: "response_format", value: "json")
    if let language = language.flatMap(Self.primaryLanguage) {
      form.add(name: "language", value: language)
    }
    let prompt = Self.prompt(from: vocabulary)
    if !prompt.isEmpty { form.add(name: "prompt", value: prompt) }
    form.add(
      name: "file", filename: "audio.wav", contentType: "audio/wav",
      data: WaveAudio.encode(samples, sampleRate: WaveAudio.recordingSampleRate))
    var request = try request("audio/transcriptions")
    request.setValue(form.contentType, forHTTPHeaderField: "Content-Type")
    request.httpBody = form.body
    let data = try await send(request)
    do {
      let answer = try JSONDecoder().decode(Transcription.self, from: data)
      return answer.text.trimmingCharacters(in: .whitespacesAndNewlines)
    } catch {
      // The decoder's message quotes the answer; the person gets the fixed one.
      throw CloudSpeechError.badResponse
    }
  }

  func speak(_ text: String, options: SpeechSynthesisOptions) async throws -> SynthesizedSpeech {
    guard let model = configuration.speechModel, configuration.speaks else {
      throw CloudSpeechError.notOffered
    }
    guard let voice = options.voice ?? configuration.voices.first else {
      throw SpeechSynthesisError.noVoice
    }
    var request = try request("audio/speech")
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.httpBody = try JSONEncoder().encode(
      SpeechBody(
        model: model, input: text, voice: voice, responseFormat: "wav",
        speed: options.speed))
    return try WaveAudio.decode(try await send(request))
  }

  /// A POST to `path` with the key, when the provider takes one. A configuration that cannot be
  /// used, or a missing key, fails before anything is sent.
  private func request(_ path: String) throws -> URLRequest {
    let key = try configuration.credential(in: vault)
    var request = URLRequest(url: configuration.endpoint(path))
    request.httpMethod = "POST"
    if let key { request.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization") }
    return request
  }

  private func send(_ request: URLRequest) async throws -> Data {
    let (data, response) = try await transport.send(request)
    if (300..<400).contains(response.statusCode) { throw CloudSpeechError.redirected }
    guard (200..<300).contains(response.statusCode) else {
      throw CloudSpeechError.rejected(response.statusCode)
    }
    return data
  }

  /// The endpoints take an ISO 639-1 code: `pt` for `pt-BR`.
  static func primaryLanguage(_ tag: String) -> String? {
    let primary = tag.split(whereSeparator: { $0 == "-" || $0 == "_" }).first.map(String.init)
    return primary?.lowercased()
  }

  static func prompt(from vocabulary: [String]) -> String {
    var prompt = ""
    for phrase in vocabulary {
      let next = prompt.isEmpty ? phrase : prompt + ", " + phrase
      if next.count > maximumPromptCharacters { break }
      prompt = next
    }
    return prompt
  }

  private struct Transcription: Decodable { let text: String }

  private struct SpeechBody: Encodable {
    let model: String
    let input: String
    let voice: String
    let responseFormat: String
    let speed: Double

    enum CodingKeys: String, CodingKey {
      case model, input, voice, speed
      case responseFormat = "response_format"
    }
  }
}

/// A `multipart/form-data` body with a boundary no field can contain.
struct MultipartForm {
  private let boundary = "soyeht-\(UUID().uuidString)"
  private var parts = Data()

  /// The fields added so far, closed.
  var body: Data { parts + Data("--\(boundary)--\r\n".utf8) }

  var contentType: String { "multipart/form-data; boundary=\(boundary)" }

  mutating func add(name: String, value: String) {
    append("--\(boundary)\r\nContent-Disposition: form-data; name=\"\(name)\"\r\n\r\n\(value)\r\n")
  }

  mutating func add(name: String, filename: String, contentType: String, data: Data) {
    append(
      "--\(boundary)\r\nContent-Disposition: form-data; name=\"\(name)\"; filename=\"\(filename)\"\r\n"
        + "Content-Type: \(contentType)\r\n\r\n")
    parts.append(data)
    append("\r\n")
  }

  private mutating func append(_ text: String) {
    parts.append(Data(text.utf8))
  }
}
