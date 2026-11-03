import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private let code = String(repeating: "a", count: 43)
private let engine = "00000000-0000-4000-8000-000000000001"
private let linkKey = String(repeating: "A", count: 43)

@MainActor @Test func reachabilityEventsReachTheClientAndThePairingSheet() throws {
  let client = EngineClient(socket: "/tmp/pairing-reachability.sock")
  #expect(client.reachability == nil)
  #expect(client.pairingGuidance.isProblem == false)

  try client.applyReachability(
    .encoded(EngineReachability(remote: false, reason: .tailnet_address_not_found)))
  #expect(client.reachability?.remote == false)
  #expect(
    client.pairingGuidance.text == "Not reachable from your devices: Tailscale address not found")
  #expect(client.pairingGuidance.isProblem)

  try client.applyReachability(.encoded(EngineReachability(remote: true)))
  #expect(client.reachability?.remote == true)
  #expect(client.pairingGuidance.isProblem == false)
}

@Test func everyUnreachableReasonExplainsItselfAndReachableSaysNothing() {
  #expect(EngineReachability(remote: true).problemText == nil)
  let reasons: [EngineReachabilityReason] = [
    .tailnet_address_not_found, .tailnet_lookup_failed, .tailnet_listen_failed, .loopback_only,
  ]
  let texts = reasons.compactMap { EngineReachability(remote: false, reason: $0).problemText }
  #expect(texts.count == reasons.count)
  #expect(Set(texts).count == reasons.count)
  #expect(texts.allSatisfy { $0.hasPrefix("Not reachable from your devices: ") })
}

@Test func aLoopbackPairingLinkIsNotReachableFromAnotherDevice() throws {
  let loopback = try PairingLink(
    "soyeht-next://pair?engineId=\(engine)&address=127.0.0.1:47321&code=\(code)&engineKey=\(linkKey)"
  )
  #expect(loopback.reachability.remote == false)
  #expect(loopback.reachability.problemText?.contains("Tailscale address not found") == true)
  let tailnet = try PairingLink(
    "soyeht-next://pair?engineId=\(engine)&address=100.64.0.9:47321&code=\(code)&engineKey=\(linkKey)"
  )
  #expect(tailnet.reachability.remote)
  let tailnetV6 = try PairingLink(
    "soyeht-next://pair?engineId=\(engine)&address=[fd7a:115c:a1e0::9]:47321&code=\(code)&engineKey=\(linkKey)"
  )
  #expect(tailnetV6.reachability.remote)
}

@Test func aCorruptVersionedLayoutFallsBackToAutomaticAndIsLoggedOnce() {
  let corrupt: JsonValue = .object(["version": .integer(2), "mode": .string("sideways")])
  let state = PaneLayoutState(workspaceLayout: corrupt)
  #expect(state == PaneLayoutState())
  // The fallback above already reported this exact layout, so a repeat stays quiet.
  #expect(
    ClientDiagnostics.reportOnce(
      "pane_layout_unusable", key: String(describing: corrupt),
      error: PaneLayoutPersistenceError.invalidTree) == false)
  // A new workspace's unversioned layout is the normal automatic case, not a failure.
  let fresh: JsonValue = .object(["panes": .array([])])
  #expect(PaneLayoutState(workspaceLayout: fresh) == PaneLayoutState())
  #expect(
    ClientDiagnostics.reportOnce(
      "pane_layout_unusable", key: String(describing: fresh),
      error: PaneLayoutPersistenceError.invalidTree))
}

@Test func anUnreadablePhoneStateStartsFreshInsteadOfFailing() {
  #expect(PhoneWritingLocalState.decoded(Data("not json".utf8)) == nil)
  #expect(ClientDiagnostics.attempt("test_attempt", key: "k") { 7 } == 7)
  let failed: Int? = ClientDiagnostics.attempt("test_attempt", key: "k") {
    throw PaneLayoutPersistenceError.invalidTree
  }
  #expect(failed == nil)
}
