import Foundation
import Observation
import SoyehtProtocol

@MainActor @Observable
public final class PairingSession {
  public private(set) var fingerprint = ""
  public private(set) var status = "Conectando ao Mac…"
  @ObservationIgnored private var wire: Wire?
  @ObservationIgnored private var earlyResult: PairResultEvent?

  public init() {}
  public func cancel() {
    wire?.close()
    wire = nil
  }

  public func pair(_ text: String, deviceName: String) async throws -> DeviceCredential {
    let link = try PairingLink(text)
    guard !deviceName.isEmpty, deviceName.utf8.count <= 128 else {
      throw ClientFailure.invalidFrame
    }
    let identity = PairingIdentity()
    fingerprint = identity.fingerprint
    let begin = PairBeginParams(
      code: link.code, deviceName: deviceName,
      devicePublicKey: identity.publicKey, idempotencyKey: UUID().uuidString)
    var prove: PairProveParams?
    let deadline = Date().addingTimeInterval(600)
    let watchdog = Task { [weak self] in
      do { try await Task.sleep(for: .seconds(600)) } catch { return }
      self?.cancel()
    }
    defer {
      watchdog.cancel()
      cancel()
    }
    for attempt in 0..<5 {
      try Task.checkCancellation()
      let wire = Wire(url: link.address)
      self.wire = wire
      earlyResult = nil
      do {
        try await wire.open()
        let first = try await wire.next()
        let greeting = try JSONDecoder().decode(JsonValue.self, from: first.payload)
        guard first.kind == 1, greeting["method"].string == "auth.challenge",
          greeting["params"]["engineId"].string == link.engineId
        else { throw ClientFailure.invalidFrame }
        let result = try await request("pair.begin", .encoded(begin)).decoded(PairBeginResult.self)
        if prove == nil {
          prove = PairProveParams(
            requestId: result.requestId,
            proof: try identity.proof(
              code: link.code, challenge: result.challenge, name: deviceName),
            idempotencyKey: UUID().uuidString)
        }
        guard prove?.requestId == result.requestId else { throw ClientFailure.invalidFrame }
        _ = try await request("pair.prove", .encoded(prove!))
        status = "Confira a impressão digital e permita o acesso no Mac."
        while earlyResult == nil {
          guard Date() < deadline else { throw ClientFailure.timeout }
          try Task.checkCancellation()
          let frame = try await wire.next()
          guard frame.kind == 1 else { throw ClientFailure.invalidFrame }
          let event = try JSONDecoder().decode(JsonValue.self, from: frame.payload)
          if event["method"].string == "pair.result" {
            earlyResult = try event["params"].decoded(PairResultEvent.self)
          }
        }
        let paired = earlyResult!
        guard paired.requestId == result.requestId else { throw ClientFailure.invalidFrame }
        guard paired.approved, let token = paired.deviceToken, let device = paired.deviceId else {
          throw ClientFailure.remote(
            "pairing_refused", paired.reason ?? "O Mac não permitiu o acesso.")
        }
        return DeviceCredential(
          engineId: link.engineId, deviceId: device, address: link.address,
          token: token, key: identity.key.rawRepresentation)
      } catch let failure as ClientFailure where !failure.canRetry { throw failure } catch {
        wire.close()
        if Task.isCancelled { throw CancellationError() }
        guard attempt < 4, Date() < deadline else { throw error }
        status = "Reconectando ao Mac…"
        try await Task.sleep(for: .seconds(1))
      }
    }
    throw ClientFailure.disconnected
  }

  private func request(_ method: String, _ params: JsonValue) async throws -> JsonValue {
    guard let wire else { throw ClientFailure.disconnected }
    let id = UUID().uuidString
    let timeout = Task { @MainActor in
      do { try await Task.sleep(for: .seconds(12)) } catch { return }
      wire.close()
    }
    defer { timeout.cancel() }
    try await wire.send(
      .object([
        "jsonrpc": .string("2.0"), "id": .string(id),
        "method": .string(method), "params": params,
      ]))
    while true {
      let frame = try await wire.next()
      guard frame.kind == 1 else { throw ClientFailure.invalidFrame }
      let message = try JSONDecoder().decode(JsonValue.self, from: frame.payload)
      guard message["jsonrpc"].string == "2.0" else { throw ClientFailure.invalidFrame }
      if message["method"].string == "pair.result" {
        earlyResult = try message["params"].decoded(PairResultEvent.self)
      }
      guard message["id"].string == id else { continue }
      if message["error"] != .null {
        throw ClientFailure.remote(
          message["error"]["data"]["kind"].string ?? "pairing_failed",
          message["error"]["message"].string ?? "Pareamento recusado.")
      }
      return message["result"]
    }
  }
}
