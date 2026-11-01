import Foundation
import Testing

@testable import SoyehtClient

private let incomingEngine = "6F1E2D3C-4B5A-4978-8A6B-5C4D3E2F1A0B"
private let otherEngine = "0C9B6A52-1B2F-4F6E-9D3A-7A1E5F4B8C10"
private let incomingCode = String(repeating: "a", count: 43)

private func link(_ address: String, engine: String = incomingEngine) -> String {
  "soyeht-next://pair?engineId=\(engine)&address=\(address)&code=\(incomingCode)"
}

/// The access a device saved when it paired with `engine` at `address`.
private func credential(engine: String, address: String) throws -> DeviceCredential {
  let json = """
    {"engineId":"\(engine)","deviceId":"d","address":"ws://\(address)","token":"t",\
    "key":"\(Data(count: 32).base64EncodedString())"}
    """
  return try JSONDecoder().decode(DeviceCredential.self, from: Data(json.utf8))
}

@Test func aLinkFromOutsideNamesWhereItWouldConnect() throws {
  let incoming = try IncomingPairing(link("100.64.0.9:8291"), replacing: nil)
  #expect(incoming.address == "100.64.0.9:8291")
  #expect(incoming.title == "Pair with the Mac at 100.64.0.9:8291?")
  #expect(incoming.link == link("100.64.0.9:8291"))
  #expect(incoming.replaced == nil)
  #expect(!incoming.message.contains("stop controlling"))
}

@Test func anIPv6HostIsWrittenInBracketsSoThePortCanBeToldApart() throws {
  let incoming = try IncomingPairing(link("[fd7a:115c:a1e0::9]:8291"), replacing: nil)
  #expect(incoming.address == "[fd7a:115c:a1e0::9]:8291")
  #expect(incoming.title == "Pair with the Mac at [fd7a:115c:a1e0::9]:8291?")
}

@Test func aLinkThatWouldReplaceAnotherMacNamesIt() throws {
  let current = try credential(engine: otherEngine, address: "[fd7a:115c:a1e0::2]:8291")
  let incoming = try IncomingPairing(link("100.64.0.9:8291"), replacing: current)
  #expect(incoming.replaced?.sameEngine == false)
  #expect(incoming.title == "Pair with the Mac at 100.64.0.9:8291?")
  #expect(
    incoming.message.hasPrefix(
      "This device will stop controlling the Mac at [fd7a:115c:a1e0::2]:8291 (engine 0C9B6A52). "))
}

@Test func aLinkForTheMacAlreadyPairedSaysSoAndOffersToPairAgain() throws {
  let current = try credential(engine: incomingEngine.lowercased(), address: "100.64.0.5:8291")
  let incoming = try IncomingPairing(link("100.64.0.9:8291"), replacing: current)
  #expect(incoming.replaced?.sameEngine == true)
  #expect(incoming.title == "Pair again with the Mac at 100.64.0.9:8291?")
  #expect(incoming.message.contains("that this device is paired with already"))
  #expect(!incoming.message.contains("stop controlling"))
}

@Test func aLinkThatIsNotAPairingLinkIsRefusedBeforeAnythingIsAsked() {
  #expect(throws: ClientFailure.self) {
    _ = try IncomingPairing("https://example.com/pair", replacing: nil)
  }
  #expect(throws: ClientFailure.self) {
    _ = try IncomingPairing(link("192.168.1.5:8291"), replacing: nil)
  }
}
