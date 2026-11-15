import Foundation

/// A cloud provider the person added: a base URL, a model and a key, over the audio endpoints
/// that OpenAI, Groq and many others share (`/audio/transcriptions` and `/audio/speech`). The
/// key is not part of it: it lives in a `ProviderKeyVault` under `id`.
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
  /// The model that transcribes, sent as `model` to `/audio/transcriptions`.
  public var transcriptionModel: String
  /// The model that speaks, sent as `model` to `/audio/speech`.
  public var speechModel: String
  /// The voices of the provider, first the default one.
  public var voices: [String]
  /// The languages offered for transcription, as BCP-47 tags.
  public var languages: [String]
  /// False for a server on the person's own network that takes no key.
  public var requiresKey: Bool

  public init(
    id: String, name: String, baseURL: URL, transcriptionModel: String, speechModel: String,
    voices: [String], languages: [String] = Self.defaultLanguages, requiresKey: Bool = true
  ) {
    self.id = id
    self.name = name
    self.baseURL = baseURL
    self.transcriptionModel = transcriptionModel
    self.speechModel = speechModel
    self.voices = voices
    self.languages = languages
    self.requiresKey = requiresKey
  }

  /// Why this configuration cannot be used, or nil. A key only travels over HTTPS; plain HTTP is
  /// for a server on this computer.
  public var problem: String? {
    guard let scheme = baseURL.scheme?.lowercased(), let host = baseURL.host, !host.isEmpty else {
      return "The address must be a full URL such as https://api.example.com/v1."
    }
    switch scheme {
    case "https": break
    case "http" where Self.isLoopback(host): break
    default: return "The address must use https unless the server runs on this computer."
    }
    if transcriptionModel.isEmpty || speechModel.isEmpty {
      return "Name the models the provider uses."
    }
    return nil
  }

  var host: String { baseURL.host ?? "" }

  func endpoint(_ name: String) -> URL { baseURL.appendingPathComponent(name) }

  private static func isLoopback(_ host: String) -> Bool {
    ["localhost", "127.0.0.1", "::1"].contains(host.lowercased())
  }
}

/// How the cloud speech providers talk to the network, so a test can answer in place of a
/// provider.
public protocol SpeechHTTPTransport: Sendable {
  func send(_ request: URLRequest) async throws -> (Data, HTTPURLResponse)
}

/// The real network: no cache, no cookies, no stored credentials.
public struct URLSessionSpeechTransport: SpeechHTTPTransport {
  private let session: URLSession

  public init() {
    let configuration = URLSessionConfiguration.ephemeral
    configuration.urlCache = nil
    configuration.httpCookieStorage = nil
    configuration.timeoutIntervalForRequest = 60
    session = URLSession(configuration: configuration)
  }

  public func send(_ request: URLRequest) async throws -> (Data, HTTPURLResponse) {
    let (data, response) = try await session.data(for: request)
    guard let http = response as? HTTPURLResponse else { throw CloudSpeechError.badResponse }
    return (data, http)
  }
}

/// What went wrong with a provider, without the provider's own text: an error body can echo part
/// of the key or the audio that was sent.
public enum CloudSpeechError: Error, LocalizedError, Equatable, Sendable {
  case invalidConfiguration(String)
  case missingKey
  case rejected(Int)
  case badResponse

  public var errorDescription: String? {
    switch self {
    case .invalidConfiguration(let problem): problem
    case .missingKey: "Add the provider's API key first."
    case .rejected(401), .rejected(403): "The provider did not accept the key."
    case .rejected(429): "The provider is limiting requests. Try again in a moment."
    case .rejected(let status): "The provider answered with an error (HTTP \(status))."
    case .badResponse: "The provider's answer could not be read."
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
    var form = MultipartForm()
    form.add(name: "model", value: configuration.transcriptionModel)
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
    let voice = options.voice ?? configuration.voices.first
    guard let voice else { throw SpeechSynthesisError.noVoice }
    var request = try request("audio/speech")
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.httpBody = try JSONEncoder().encode(
      SpeechBody(
        model: configuration.speechModel, input: text, voice: voice, responseFormat: "wav",
        speed: options.speed))
    return try WaveAudio.decode(try await send(request))
  }

  /// A POST to `path` with the key, when the provider takes one. A configuration that cannot be
  /// used, or a missing key, fails before anything is sent.
  private func request(_ path: String) throws -> URLRequest {
    if let problem = configuration.problem {
      throw CloudSpeechError.invalidConfiguration(problem)
    }
    var request = URLRequest(url: configuration.endpoint(path))
    request.httpMethod = "POST"
    if configuration.requiresKey {
      guard let key = try vault.key(for: configuration.id), !key.isEmpty else {
        throw CloudSpeechError.missingKey
      }
      request.setValue("Bearer \(key)", forHTTPHeaderField: "Authorization")
    }
    return request
  }

  private func send(_ request: URLRequest) async throws -> Data {
    let (data, response) = try await transport.send(request)
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
