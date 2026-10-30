import Foundation
import SoyehtProtocol
import Testing
import os

@testable import SoyehtClient

// The fetch of the update check follows at most `UpdatePolicy.maxRedirects` redirects and none that
// leaves HTTPS (contract 15, section 6). A `URLProtocol` stub answers from a table, so no test
// reaches the network; each test uses a host of its own, because the table is shared.

private enum Answer {
  case redirect(String)
  case body(Data)
}

private struct Table {
  var answers: [String: Answer] = [:]
  var requests: [String] = []
  var userAgents: [String: String] = [:]
}

private let table = OSAllocatedUnfairLock(initialState: Table())

private final class StubProtocol: URLProtocol, @unchecked Sendable {
  override class func canInit(with request: URLRequest) -> Bool { true }
  override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

  override func startLoading() {
    guard let url = request.url else { return }
    let key = url.absoluteString
    let agent = request.value(forHTTPHeaderField: "User-Agent")
    let answer = table.withLock { state -> Answer? in
      state.requests.append(key)
      state.userAgents[key] = agent
      return state.answers[key]
    }
    switch answer {
    case .redirect(let location)?:
      guard let target = URL(string: location),
        let response = HTTPURLResponse(
          url: url, statusCode: 302, httpVersion: nil, headerFields: ["Location": location])
      else { return }
      client?.urlProtocol(
        self, wasRedirectedTo: URLRequest(url: target), redirectResponse: response)
      // A session that does not follow the redirect gets the redirect itself as the answer, as it
      // does from a server; one that follows stops this load first and ignores these two calls.
      client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
      client?.urlProtocolDidFinishLoading(self)
    case .body(let data)?:
      guard
        let response = HTTPURLResponse(
          url: url, statusCode: 200, httpVersion: nil, headerFields: nil)
      else { return }
      client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
      client?.urlProtocol(self, didLoad: data)
      client?.urlProtocolDidFinishLoading(self)
    case nil:
      client?.urlProtocol(self, didFailWithError: URLError(.cannotFindHost))
    }
  }

  override func stopLoading() {}
}

private func stubbed() -> URLSessionConfiguration {
  let configuration = URLSessionConfiguration.ephemeral
  configuration.protocolClasses = [StubProtocol.self]
  return configuration
}

/// `https://host/0` redirects to `/1` and so on; the last of the `hops` redirects to `/hops`,
/// which answers with `body`. Returns the first address.
private func chain(host: String, hops: Int, body: String = "manifest") -> URL {
  table.withLock { state in
    for step in 0..<hops {
      state.answers["https://\(host)/\(step)"] = .redirect("https://\(host)/\(step + 1)")
    }
    state.answers["https://\(host)/\(hops)"] = .body(Data(body.utf8))
  }
  return URL(string: "https://\(host)/0")!
}

private func requests(to host: String) -> [String] {
  table.withLock { $0.requests.filter { $0.contains("//\(host)/") } }
}

@Test func theFetchFollowsRedirectsUpToTheContractsLimit() async {
  let address = chain(host: "within.redirects.test", hops: UpdatePolicy.maxRedirects)
  let result = await fetchManifest(address, configuration: stubbed())
  #expect(result == .success("manifest"))
  #expect(requests(to: "within.redirects.test").count == UpdatePolicy.maxRedirects + 1)
}

@Test func theFetchRefusesTheRedirectPastTheLimitWithoutAskingForItsTarget() async {
  let address = chain(host: "past.redirects.test", hops: UpdatePolicy.maxRedirects + 1)
  let result = await fetchManifest(address, configuration: stubbed())
  #expect(result == .failure(.unreachable))
  let asked = requests(to: "past.redirects.test")
  #expect(asked.count == UpdatePolicy.maxRedirects + 1)
  #expect(!asked.contains("https://past.redirects.test/\(UpdatePolicy.maxRedirects + 1)"))
}

@Test func theFetchRefusesARedirectThatLeavesHTTPS() async {
  table.withLock { state in
    state.answers["https://downgrade.redirects.test/0"] = .redirect(
      "http://downgrade.redirects.test/plain")
    state.answers["http://downgrade.redirects.test/plain"] = .body(Data("manifest".utf8))
  }
  let address = URL(string: "https://downgrade.redirects.test/0")!
  let result = await fetchManifest(address, configuration: stubbed())
  #expect(result == .failure(.unreachable))
  #expect(requests(to: "downgrade.redirects.test") == ["https://downgrade.redirects.test/0"])
}

@Test func theFetchNeverStartsOnAnAddressThatIsNotHTTPS() async {
  table.withLock { $0.answers["http://plain.redirects.test/0"] = .body(Data("manifest".utf8)) }
  let result = await fetchManifest(
    URL(string: "http://plain.redirects.test/0")!, configuration: stubbed())
  #expect(result == .failure(.unreachable))
  #expect(requests(to: "plain.redirects.test").isEmpty)
}

@Test func theFetchSendsTheContractsUserAgentAndRefusesABodyOverTheLimit() async {
  let big = String(repeating: "a", count: UpdatePolicy.manifestMaxBytes + 1)
  let address = chain(host: "limits.redirects.test", hops: 0, body: big)
  let result = await fetchManifest(address, configuration: stubbed())
  #expect(result == .failure(.tooLarge))
  let agent = table.withLock { $0.userAgents["https://limits.redirects.test/0"] }
  #expect(agent == UpdatePolicy.userAgent)
}
