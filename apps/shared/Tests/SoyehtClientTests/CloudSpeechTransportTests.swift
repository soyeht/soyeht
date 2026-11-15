import Foundation
import Testing

@testable import SoyehtClient

/// Answers requests in place of the network. A test names its own host, so tests may run
/// together; nothing here reaches a real server.
final class StubURLProtocol: URLProtocol, @unchecked Sendable {
  struct Reply {
    var status = 200
    var headers: [String: String] = [:]
    var body = Data()
    var redirectTo: URL?
    /// Never answers: the request stays open until its deadline.
    var hangs = false
  }

  private static let lock = NSLock()
  private nonisolated(unsafe) static var replies: [String: Reply] = [:]
  private nonisolated(unsafe) static var seen: [URL] = []

  static func answer(host: String, with reply: Reply) {
    lock.withLock { replies[host] = reply }
  }

  static func requests(to host: String) -> [URL] {
    lock.withLock { seen.filter { $0.host == host } }
  }

  /// Only the hosts of these tests: a request to anything else is not answered here.
  override class func canInit(with request: URLRequest) -> Bool {
    request.url?.host?.hasSuffix(".test") == true
  }

  override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

  override func startLoading() {
    guard let url = request.url, let host = url.host else { return }
    let reply = Self.lock.withLock { () -> Reply? in
      Self.seen.append(url)
      return Self.replies[host]
    }
    guard let reply,
      let response = HTTPURLResponse(
        url: url, statusCode: reply.status, httpVersion: nil, headerFields: reply.headers)
    else {
      client?.urlProtocol(self, didFailWithError: URLError(.cannotFindHost))
      return
    }
    if reply.hangs { return }
    if let target = reply.redirectTo {
      client?.urlProtocol(
        self, wasRedirectedTo: URLRequest(url: target), redirectResponse: response)
      // When the session's delegate declines, the redirect response is the answer, and the
      // protocol delivers it below; when the redirect is followed the load is stopped instead.
    }
    client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
    client?.urlProtocol(self, didLoad: reply.body)
    client?.urlProtocolDidFinishLoading(self)
  }

  override func stopLoading() {}
}

private func transport(
  limit: Int? = nil, baseSeconds: TimeInterval? = nil
) -> URLSessionSpeechTransport {
  let configuration = URLSessionConfiguration.ephemeral
  configuration.protocolClasses = [StubURLProtocol.self]
  return URLSessionSpeechTransport(
    configuration: configuration, maximumResponseBytes: limit, baseSeconds: baseSeconds)
}

private func post(_ host: String) -> URLRequest {
  var request = URLRequest(url: URL(string: "https://\(host)/v1/audio/speech")!)
  request.httpMethod = "POST"
  return request
}

@Test func theRealTransportReturnsTheStatusAndTheBody() async throws {
  StubURLProtocol.answer(host: "ok.transport.test", with: .init(body: Data("hello".utf8)))
  let (data, response) = try await transport().send(post("ok.transport.test"))
  #expect(response.statusCode == 200)
  #expect(String(decoding: data, as: UTF8.self) == "hello")
}

@Test func theSessionHasADeadlineForTheWholeRequestAndKeepsNothing() {
  let configuration = transport().sessionConfiguration
  #expect(configuration.timeoutIntervalForResource == URLSessionSpeechTransport.ceilingSeconds)
  #expect(configuration.timeoutIntervalForRequest == URLSessionSpeechTransport.idleSeconds)
  #expect(configuration.urlCache == nil)
  #expect(configuration.httpCookieStorage == nil)
  #expect(configuration.urlCredentialStorage == nil)
}

@Test func anAnswerBeyondTheLimitIsRefusedWhileItIsReceived() async {
  StubURLProtocol.answer(
    host: "big.transport.test", with: .init(body: Data(repeating: 1, count: 5_000)))
  await #expect(throws: CloudSpeechError.responseTooLarge) {
    try await transport(limit: 1_000).send(post("big.transport.test"))
  }
}

@Test func anAnswerThatDeclaresMoreThanTheLimitIsRefusedBeforeItsBody() async {
  StubURLProtocol.answer(
    host: "declared.transport.test",
    with: .init(headers: ["Content-Length": "5000"], body: Data(repeating: 1, count: 10)))
  await #expect(throws: CloudSpeechError.self) {
    try await transport(limit: 1_000).send(post("declared.transport.test"))
  }
}

@Test func anAnswerExactlyAtTheLimitIsKept() async throws {
  StubURLProtocol.answer(
    host: "exact.transport.test", with: .init(body: Data(repeating: 1, count: 1_000)))
  let (data, _) = try await transport(limit: 1_000).send(post("exact.transport.test"))
  #expect(data.count == 1_000)
}

@Test func aRedirectIsNotFollowedSoNothingIsSentToAnotherHost() async throws {
  StubURLProtocol.answer(
    host: "from.transport.test",
    with: .init(
      status: 302, headers: ["Location": "http://elsewhere.transport.test/v1"],
      redirectTo: URL(string: "http://elsewhere.transport.test/v1")))
  StubURLProtocol.answer(host: "elsewhere.transport.test", with: .init(body: Data("stolen".utf8)))
  let (data, response) = try await transport().send(post("from.transport.test"))
  #expect((300..<400).contains(response.statusCode))
  #expect(String(decoding: data, as: UTF8.self) != "stolen")
  #expect(StubURLProtocol.requests(to: "elsewhere.transport.test").isEmpty)
}

@Test func theRequestsOfAProviderReportTheRedirectTheTransportRefused() async throws {
  StubURLProtocol.answer(
    host: "speech.redirect.test",
    with: .init(
      status: 301, headers: ["Location": "http://other.redirect.test/"],
      redirectTo: URL(string: "http://other.redirect.test/")))
  let configuration = cloudConfiguration(base: "https://speech.redirect.test/v1")
  let requests = CloudSpeechRequests(
    configuration: configuration, vault: MemoryProviderKeyVault([configuration.id: "sk-test-1"]),
    transport: transport())
  await #expect(throws: CloudSpeechError.redirected) {
    try await requests.transcribe([0], language: nil, vocabulary: [])
  }
  #expect(StubURLProtocol.requests(to: "other.redirect.test").isEmpty)
}

@Test func theDeadlineGrowsWithWhatIsUploadedUpToACeiling() {
  // 120 s, plus one second for every 64 KiB of the body, and never more than an hour.
  #expect(URLSessionSpeechTransport.deadline(forBodyBytes: 0) == 120)
  #expect(URLSessionSpeechTransport.deadline(forBodyBytes: 64 << 10) == 121)
  // Ten minutes of 16 kHz 16-bit audio: about 19.2 MB, so about seven minutes.
  let recording = URLSessionSpeechTransport.deadline(forBodyBytes: 19_200_000)
  #expect(recording > 400 && recording < 430)
  #expect(URLSessionSpeechTransport.deadline(forBodyBytes: Int.max / 2) == 3_600)
}

@Test func aRequestThatNeverAnswersEndsAtItsDeadlineAndSaysSo() async {
  StubURLProtocol.answer(host: "silent.transport.test", with: .init(hangs: true))
  await #expect(throws: CloudSpeechError.timedOut) {
    try await transport(baseSeconds: 0.3).send(post("silent.transport.test"))
  }
}
