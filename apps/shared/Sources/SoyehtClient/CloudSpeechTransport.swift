import Foundation

/// How the cloud speech providers talk to the network, so a test can answer in place of a
/// provider.
public protocol SpeechHTTPTransport: Sendable {
  func send(_ request: URLRequest) async throws -> (Data, HTTPURLResponse)
}

/// The real network, for one provider: no cache, no cookies, no stored credentials, a deadline
/// for the whole request that grows with the size of what is uploaded, a limit on the answer's
/// size, and no redirect followed. The address is
/// one the person entered, so a recording, a text or a key is never sent anywhere else.
///
/// It owns one session for its life and closes it when it goes away.
public final class URLSessionSpeechTransport: SpeechHTTPTransport, @unchecked Sendable {
  /// A speech answer is a few megabytes; a transcript is a few kilobytes.
  static let maximumResponseBytes = 16 << 20
  /// The deadline of a request, its upload and its answer, from start to end, is `baseSeconds`
  /// plus one second for every `uploadBytesPerSecond` of the body (an uplink of 64 KiB/s, about
  /// half a megabit, is assumed at the least), and never more than `ceilingSeconds`. A ten-minute
  /// recording is about 19 MB and so gets about seven minutes; a text, two.
  static let baseSeconds: TimeInterval = 120
  static let uploadBytesPerSecond = 64 << 10
  static let ceilingSeconds: TimeInterval = 3_600

  static func deadline(
    forBodyBytes bytes: Int, baseSeconds: TimeInterval = URLSessionSpeechTransport.baseSeconds
  ) -> TimeInterval {
    min(ceilingSeconds, baseSeconds + Double(bytes) / Double(uploadBytesPerSecond))
  }
  /// How long the connection may be silent.
  static let idleSeconds: TimeInterval = 60

  private let session: URLSession
  private let maximumResponseBytes: Int
  private let baseSeconds: TimeInterval

  public convenience init() {
    self.init(configuration: .ephemeral, maximumResponseBytes: nil, baseSeconds: nil)
  }

  /// A session made from `configuration`, with the limits above applied on top of it. A test
  /// passes one with its own `protocolClasses`.
  init(
    configuration: URLSessionConfiguration, maximumResponseBytes: Int?,
    baseSeconds: TimeInterval? = nil
  ) {
    self.maximumResponseBytes = maximumResponseBytes ?? Self.maximumResponseBytes
    self.baseSeconds = baseSeconds ?? Self.baseSeconds
    configuration.urlCache = nil
    configuration.httpCookieStorage = nil
    configuration.urlCredentialStorage = nil
    configuration.timeoutIntervalForRequest = Self.idleSeconds
    // The session's own limit is only the ceiling: each request is held to its own deadline.
    configuration.timeoutIntervalForResource = Self.ceilingSeconds
    session = URLSession(
      configuration: configuration, delegate: RefuseRedirects(), delegateQueue: nil)
  }

  /// The configuration of the session, for a test to check the limits.
  var sessionConfiguration: URLSessionConfiguration { session.configuration }

  deinit { session.invalidateAndCancel() }

  public func send(_ request: URLRequest) async throws -> (Data, HTTPURLResponse) {
    let seconds = Self.deadline(
      forBodyBytes: request.httpBody?.count ?? 0, baseSeconds: baseSeconds)
    return try await withThrowingTaskGroup(of: (Data, HTTPURLResponse).self) { group in
      group.addTask { try await self.receive(request) }
      group.addTask {
        try await Task.sleep(for: .seconds(seconds))
        throw CloudSpeechError.timedOut
      }
      defer { group.cancelAll() }
      guard let answer = try await group.next() else { throw CloudSpeechError.badResponse }
      return answer
    }
  }

  private func receive(_ request: URLRequest) async throws -> (Data, HTTPURLResponse) {
    let (bytes, response) = try await session.bytes(for: request)
    guard let http = response as? HTTPURLResponse else { throw CloudSpeechError.badResponse }
    let limit = maximumResponseBytes
    if response.expectedContentLength > Int64(limit) { throw CloudSpeechError.responseTooLarge }
    var data = Data()
    data.reserveCapacity(max(0, min(Int(response.expectedContentLength), limit)))
    for try await byte in bytes {
      data.append(byte)
      if data.count > limit { throw CloudSpeechError.responseTooLarge }
    }
    return (data, http)
  }

  /// Answers every redirect with "do not follow": the task then ends with the redirect response
  /// itself, which the caller reports.
  private final class RefuseRedirects: NSObject, URLSessionTaskDelegate {
    func urlSession(
      _ session: URLSession, task: URLSessionTask,
      willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
      completionHandler: @escaping (URLRequest?) -> Void
    ) {
      completionHandler(nil)
    }
  }
}
