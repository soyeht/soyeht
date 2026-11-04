import Foundation
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

/// The list of small-order points in `PairingLink` is the vector's, so the two cannot drift apart.
@Test func everyWeakEngineKeyOfTheVectorIsRefusedAsAnEngineKey() throws {
  let vector = try ContractVector.load("pairing-link")
  let keys = try #require(vector["weakEngineKeys"].array)
  #expect(keys.count == 8)
  for key in keys {
    let hex = key.string ?? ""
    let bytes = stride(from: 0, to: hex.count, by: 2).map { offset -> UInt8 in
      let start = hex.index(hex.startIndex, offsetBy: offset)
      return UInt8(hex[start..<hex.index(start, offsetBy: 2)], radix: 16) ?? 0
    }
    let text = Data(bytes).base64EncodedString()
      .replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_")
      .replacingOccurrences(of: "=", with: "")
    let link =
      "soyeht-next://pair?engineId=6f1e2d3c-4b5a-4978-8a6b-5c4d3e2f1a0b&address=127.0.0.1:9"
      + "&code=" + String(repeating: "A", count: 43) + "&engineKey=" + text
    #expect(throws: ClientFailure.self, "\(hex)") { _ = try PairingLink(link) }
  }
}
