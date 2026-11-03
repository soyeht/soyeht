import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/pairing-link.json`, which `soyeht-protocol` passes as well: what a
/// pairing link must carry and which addresses it may name (ADR 0134).
@Test func everyCaseOfThePairingLinkVectorGivesTheSameAnswer() throws {
  let vector = try ContractVector.load("pairing-link")
  let cases = try #require(vector["cases"].array)
  #expect(cases.count >= 10)
  for item in cases {
    let name = item["name"].string ?? ""
    let text = item["link"].string ?? ""
    switch item["result"].string {
    case "valid":
      let link = try PairingLink(text)
      let host = link.address.host ?? ""
      let shown = host.contains(":") ? "[\(host)]" : host
      #expect("\(shown):\(link.address.port ?? 0)" == item["address"].string, "\(name)")
      #expect(link.engineId.lowercased() == item["engineId"].string, "\(name)")
    case "old":
      do {
        _ = try PairingLink(text)
        Issue.record("\(name): accepted")
      } catch ClientFailure.remote(let kind, _, _) {
        #expect(kind == ErrorKind.protocol_version_unsupported.rawValue, "\(name)")
      }
    default:
      #expect(throws: ClientFailure.self, "\(name)") { _ = try PairingLink(text) }
    }
  }
}
