import Testing

@testable import SoyehtClient

private let incomingEngine = "6F1E2D3C-4B5A-4978-8A6B-5C4D3E2F1A0B"
private let incomingCode = String(repeating: "a", count: 43)

private func link(_ address: String) -> String {
  "soyeht-next://pair?engineId=\(incomingEngine)&address=\(address)&code=\(incomingCode)"
}

@Test func aLinkFromOutsideNamesWhereItWouldConnect() throws {
  let incoming = try IncomingPairing(link("100.64.0.9:8291"), replacingMac: false)
  #expect(incoming.address == "100.64.0.9:8291")
  #expect(incoming.title == "Pair with the Mac at 100.64.0.9:8291?")
  #expect(incoming.link == link("100.64.0.9:8291"))
}

@Test func aLinkThatWouldReplaceTheMacSaysSo() throws {
  let first = try IncomingPairing(link("100.64.0.9:8291"), replacingMac: false)
  let replacing = try IncomingPairing(link("100.64.0.9:8291"), replacingMac: true)
  #expect(!first.message.contains("stop controlling"))
  #expect(replacing.message.contains("stop controlling the Mac it is paired with"))
}

@Test func aLinkThatIsNotAPairingLinkIsRefusedBeforeAnythingIsAsked() {
  #expect(throws: ClientFailure.self) {
    _ = try IncomingPairing("https://example.com/pair", replacingMac: false)
  }
  #expect(throws: ClientFailure.self) {
    _ = try IncomingPairing(link("192.168.1.5:8291"), replacingMac: false)
  }
}
