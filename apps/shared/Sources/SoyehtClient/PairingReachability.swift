import Foundation
import SoyehtProtocol

extension EngineReachability {
  /// What the Mac and the phone show when paired devices cannot reach the engine.
  public var problemText: String? {
    guard !remote else { return nil }
    let detail =
      switch reason {
      case .tailnet_address_not_found, nil: "Tailscale address not found"
      case .tailnet_lookup_failed: "the network interfaces could not be read"
      case .tailnet_listen_failed: "Soyeht could not listen on the Tailscale address"
      case .loopback_only: "this Mac listens on loopback only"
      }
    return "Not reachable from your devices: \(detail)"
  }
}

extension PairingLink {
  /// A loopback link only works on the Mac itself: the engine had no tailnet address
  /// when the code was made.
  public var reachability: EngineReachability {
    let host = address.host?.trimmingCharacters(in: CharacterSet(charactersIn: "[]")) ?? ""
    let loopback = host.hasPrefix("127.") || host == "::1"
    return EngineReachability(
      remote: !loopback, reason: loopback ? .tailnet_address_not_found : nil)
  }
}

extension EngineClient {
  /// The pairing sheet's instruction, or why paired devices cannot reach this Mac.
  public var pairingGuidance: (text: String, isProblem: Bool) {
    if let problem = reachability?.problemText { return (problem, true) }
    return (
      "Open Soyeht Dev on iPhone and scan this code.\nBoth devices must be on the same tailnet.",
      false
    )
  }

  public func beginPairing() {
    perform {
      let code = try await self.mutation(
        .devicePairStart,
        DevicePairStartParams(idempotencyKey: UUID().uuidString)
      ).decoded(DevicePairStartResult.self)
      self.pairCode = code
      if let reachability = code.reachability { self.reachability = reachability }
    }
  }
  public func revokeDevice(_ id: String) {
    perform {
      _ = try await self.mutation(
        .deviceRevoke, DeviceRevokeParams(deviceId: id, idempotencyKey: UUID().uuidString))
    }
  }
  public func clearPairCode() { pairCode = nil }
  public func decidePairing(_ request: PairPrompt, approve: Bool) {
    perform {
      _ = try await self.mutation(
        .devicePairDecide,
        DevicePairDecideParams(
          requestId: request.requestId, approve: approve, idempotencyKey: UUID().uuidString))
      self.pairRequests.removeAll { $0.requestId == request.requestId }
      if approve { self.pairCode = nil }
    }
  }

  func applyReachability(_ params: JsonValue) throws {
    let reachability = try params.decoded(EngineReachability.self)
    self.reachability = reachability
    // A code made while unreachable carries the loopback address. Once the tailnet is
    // back, replace it so the phone can dial the Mac.
    if reachability.remote, pairCode?.reachability?.remote == false { beginPairing() }
  }
}
