import CryptoKit
import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// The cases of `contracts/vectors/engine-proof.json`, which the engine passes as well (ADR 0132).
private struct Proofs {
  let vector = try! ContractVector.load("engine-proof")
  func text(_ name: String) -> String { vector[name].string ?? "" }
  func data(_ name: String) -> Data { Data(base64Encoded: text(name)) ?? Data() }
  var engineKey: Data { data("enginePublicKey") }
  /// What the engine signs in `welcome`, from the vector's nonces and identities.
  var engineHelloSigned: Data {
    EngineVerification.helloBytes(
      nonce: data("nonce"), clientNonce: data("clientNonce"), engineId: text("engineId"),
      deviceId: text("deviceId"))
  }
}

private func welcome(proof: String?) throws -> Welcome {
  var object: [String: JsonValue] = [
    "engineVersion": .string("1"), "protocolVersion": .integer(1), "connectionId": .string("c"),
    "role": .string("human_device"), "capabilities": .array([]), "dbId": .string("d"),
  ]
  if let proof { object["engineProof"] = .string(proof) }
  return try JsonValue.object(object).decoded(Welcome.self)
}

@Test func theDeviceSignsWhatTheVectorSays() throws {
  let proofs = Proofs()
  let device = try Curve25519.Signing.PrivateKey(rawRepresentation: proofs.data("deviceSeed"))
  let signed = DeviceCredential.helloBytes(
    nonce: proofs.data("nonce"), clientNonce: proofs.data("clientNonce"),
    engineId: proofs.text("engineId"), token: proofs.text("deviceToken"))
  // CryptoKit's signatures are randomized: the vector's signature is checked, not compared.
  #expect(
    device.publicKey.isValidSignature(proofs.data("helloProof"), for: signed),
    "the bytes the device signs are those of the vector")
  #expect(device.publicKey.rawRepresentation == proofs.data("devicePublicKey"))
}

@Test func theEnginesProofOfTheVectorIsTheOneTheDeviceChecks() throws {
  let proofs = Proofs()
  let signed = proofs.engineHelloSigned
  let check = EngineVerification(engineKey: proofs.engineKey, signed: signed)
  try check.check(welcome(proof: proofs.text("engineHelloProof")))
}

@Test func anEngineThatCannotSignIsRefusedAndTheReasonIsSaid() throws {
  let proofs = Proofs()
  let signed = proofs.engineHelloSigned
  let check = EngineVerification(engineKey: proofs.engineKey, signed: signed)
  func refusal(_ proof: String?) -> String? {
    do {
      try check.check(welcome(proof: proof))
      return nil
    } catch ClientFailure.remote(let kind, _, _) { return kind } catch { return "other" }
  }
  // An impostor signs with a key of its own; a relay of an old answer carries other nonces; the
  // pairing proof is not a hello proof; an engine that sends none is too old to be checked.
  let impostor = Curve25519.Signing.PrivateKey()
  let forged = try impostor.signature(for: signed).base64EncodedString()
  #expect(refusal(forged) == ErrorKind.engine_identity_changed.rawValue)
  #expect(refusal(proofs.text("enginePairProof")) == ErrorKind.engine_identity_changed.rawValue)
  #expect(refusal("not base64 !") == ErrorKind.protocol_version_unsupported.rawValue)
  #expect(refusal(nil) == ErrorKind.protocol_version_unsupported.rawValue)
}

@Test func theEnginesPairingProofBindsTheRequestTheChallengeAndTheDevice() throws {
  let proofs = Proofs()
  let device = try Curve25519.Signing.PrivateKey(rawRepresentation: proofs.data("deviceSeed"))
  let signed =
    Data("soyeht-engine-pair-v1".utf8) + proofs.data("challenge")
    + device.publicKey.rawRepresentation + Data(proofs.text("requestId").utf8)
  try EngineVerification.verify(
    proofs.text("enginePairProof"), signed: signed, engineKey: proofs.engineKey, missing: "")
  #expect(throws: ClientFailure.self) {
    try EngineVerification.verify(
      proofs.text("enginePairProof"), signed: signed + Data([1]), engineKey: proofs.engineKey,
      missing: "")
  }
}

@Test func aCredentialSavedBeforeDevicesCheckedTheEngineIsRefusedWithAnInstruction() throws {
  let proofs = Proofs()
  let old = """
    {"engineId":"\(proofs.text("engineId"))","deviceId":"\(proofs.text("deviceId"))",\
    "address":"ws://100.64.0.9:8291","token":"t","key":"\(proofs.text("deviceSeed"))"}
    """
  let credential = try JSONDecoder().decode(DeviceCredential.self, from: Data(old.utf8))
  let challenge = AuthChallengeEvent(
    nonce: proofs.text("nonce"), engineId: proofs.text("engineId"))
  do {
    _ = try credential.hello(challenge: challenge)
    Issue.record("a credential with no engine key must not connect")
  } catch ClientFailure.remote(let kind, let message, _) {
    #expect(kind == ErrorKind.engine_identity_changed.rawValue)
    #expect(message.contains("Pair again"))
  }
}
