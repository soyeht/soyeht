import CryptoKit
import Foundation
import Network
import SoyehtProtocol

public struct PairingLink: Sendable {
  public let engineId: String
  public let address: URL
  let code: String
  /// The engine's Ed25519 public key, 32 bytes, which the link carries so the device pins it.
  let engineKey: Data
  public init(_ text: String) throws {
    // A Mac that has not been updated writes the three fields of the old link: say what to do.
    if let old = URLComponents(string: text), old.scheme == "soyeht-next", old.host == "pair",
      Set((old.queryItems ?? []).map(\.name)) == Set(["engineId", "address", "code"])
    {
      throw ClientFailure.remote(
        ErrorKind.protocol_version_unsupported.rawValue,
        "Update Soyeht on the Mac, then pair with its new QR code.")
    }
    guard let parts = URLComponents(string: text), parts.scheme == "soyeht-next",
      parts.host == "pair", parts.path.isEmpty, parts.fragment == nil,
      let items = parts.queryItems, items.count == 4,
      Set(items.map(\.name)) == Set(["engineId", "address", "code", "engineKey"]),
      let engine = items.first(where: { $0.name == "engineId" })?.value,
      UUID(uuidString: engine) != nil,
      let code = items.first(where: { $0.name == "code" })?.value,
      code.utf8.count == 43,
      code.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-" || $0 == "_") }),
      let keyText = items.first(where: { $0.name == "engineKey" })?.value,
      let engineKey = Self.keyBytes(keyText),
      let address = items.first(where: { $0.name == "address" })?.value,
      let endpoint = URLComponents(string: "ws://" + address),
      endpoint.user == nil, endpoint.password == nil, endpoint.path.isEmpty,
      endpoint.query == nil, endpoint.fragment == nil,
      let port = endpoint.port, (1...65535).contains(port), let host = endpoint.host,
      Self.permittedHost(host), let url = endpoint.url
    else {
      throw ClientFailure.remote(
        "invalid_pairing_code", "This QR is not a valid Soyeht Dev pairing.")
    }
    self.engineId = engine
    self.code = code
    self.engineKey = engineKey
    self.address = url
  }
  /// The 32 bytes of a key written as URL-safe base64 without padding (43 characters).
  static func keyBytes(_ text: String) -> Data? {
    guard text.utf8.count == 43,
      text.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-" || $0 == "_") })
    else { return nil }
    let standard = text.replacingOccurrences(of: "-", with: "+").replacingOccurrences(
      of: "_", with: "/")
    guard let bytes = Data(base64Encoded: standard + "="), bytes.count == 32 else { return nil }
    return bytes
  }
  static func permittedHost(_ source: String) -> Bool {
    let host = source.trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
    if let ip = IPv4Address(host) {
      let bytes = Array(ip.rawValue)
      return bytes[0] == 127 || (bytes[0] == 100 && (64...127).contains(bytes[1]))
    }
    if let ip = IPv6Address(host) {
      let bytes = Array(ip.rawValue)
      return ip == IPv6Address("::1") || bytes.prefix(6) == [0xfd, 0x7a, 0x11, 0x5c, 0xa1, 0xe0]
    }
    return false
  }
}

public struct DeviceCredential: Codable, Sendable {
  public let engineId: String
  public let deviceId: String
  public let address: URL
  let token: String
  private let key: Data
  /// The engine's public key from the pairing link. A credential saved before devices checked the
  /// engine has none, and is refused with the instruction to pair again.
  let engineKey: Data?

  init(
    engineId: String, deviceId: String, address: URL, token: String, key: Data, engineKey: Data
  ) {
    self.engineId = engineId
    self.deviceId = deviceId
    self.address = address
    self.token = token
    self.key = key
    self.engineKey = engineKey
  }
  /// What the device signs in `hello`.
  static func helloBytes(nonce: Data, clientNonce: Data, engineId: String, token: String) -> Data {
    Data("soyeht-hello-v2".utf8) + nonce + clientNonce + Data(engineId.utf8) + Data(token.utf8)
  }
  /// The `hello` for this connection, and what the engine's `welcome` has to prove. The device
  /// signs both nonces; the engine signs them back with the key the pairing link named, so a
  /// server that does not hold that key is refused (ADR 0132).
  func hello(challenge: AuthChallengeEvent) throws -> (HelloParams, EngineVerification) {
    guard challenge.engineId == engineId, let nonce = Data(base64Encoded: challenge.nonce),
      nonce.count == 32
    else {
      throw ClientFailure.remote(
        ErrorKind.engine_identity_changed.rawValue, "The Mac identity changed. Pair again.")
    }
    guard let engineKey else {
      throw ClientFailure.remote(
        ErrorKind.engine_identity_changed.rawValue,
        "This device cannot check that it is talking to your Mac. Pair again.")
    }
    var clientNonce = Data(count: 32)
    for index in clientNonce.indices { clientNonce[index] = UInt8.random(in: 0...255) }
    let key = try Curve25519.Signing.PrivateKey(rawRepresentation: key)
    let bytes = Self.helloBytes(
      nonce: nonce, clientNonce: clientNonce, engineId: engineId, token: token)
    let params = HelloParams.iphone(
      deviceToken: token, proof: try key.signature(for: bytes).base64EncodedString(),
      clientNonce: clientNonce.base64EncodedString())
    let verification = EngineVerification(
      engineKey: engineKey,
      signed: EngineVerification.helloBytes(
        nonce: nonce, clientNonce: clientNonce, engineId: engineId, deviceId: deviceId))
    return (params, verification)
  }
}

/// The check of the engine's answer to a `hello`: its `welcome` must carry a signature, by the key
/// the pairing link named, of this connection's two nonces and the two identities.
struct EngineVerification: Sendable {
  let engineKey: Data
  let signed: Data

  static func helloBytes(nonce: Data, clientNonce: Data, engineId: String, deviceId: String)
    -> Data
  {
    Data("soyeht-engine-v1".utf8) + nonce + clientNonce + Data(engineId.utf8) + Data([0])
      + Data(deviceId.utf8)
  }

  func check(_ welcome: Welcome) throws {
    try Self.verify(
      welcome.engineProof, signed: signed, engineKey: engineKey,
      missing: "This Mac cannot prove who it is. Update Soyeht on the Mac.")
  }

  /// Fails with an explanation when `proof` is absent, or not the engine's signature of `signed`.
  static func verify(_ proof: String?, signed: Data, engineKey: Data, missing: String) throws {
    guard let proof else {
      throw ClientFailure.remote(ErrorKind.protocol_version_unsupported.rawValue, missing)
    }
    let verified: Bool
    do {
      verified = try Curve25519.Signing.PublicKey(rawRepresentation: engineKey)
        .isValidSignature(Data(base64Encoded: proof) ?? Data(), for: signed)
    } catch {
      // A key that is not a key verifies nothing: the engine did not prove itself.
      verified = false
    }
    guard verified else {
      throw ClientFailure.remote(Self.notYourMac, Self.notYourMacText)
    }
  }

  /// The kind of a proof that does not verify. Unlike `engine_identity_changed` it does not stop
  /// the reconnecting: a server at a stale or borrowed address must not disable a real phone.
  static let notYourMac = "not_your_mac"
  static let notYourMacText =
    "This is not your Mac: the server at this address could not prove it holds the key this device "
    + "trusts. Trying again."
}

extension ClientFailure {
  /// Whether a refusal of `hello` means the saved access is no good, so reconnecting is pointless
  /// until the person pairs again. A server that fails to prove itself is not such a refusal.
  static func refusesCredential(kind: String) -> Bool {
    [ErrorKind.unauthorized.rawValue, ErrorKind.engine_identity_changed.rawValue].contains(kind)
  }
}

struct PairingIdentity {
  let key = Curve25519.Signing.PrivateKey()
  var publicKey: String { key.publicKey.rawRepresentation.base64EncodedString() }
  var fingerprint: String {
    SHA256.hash(data: key.publicKey.rawRepresentation).map { String(format: "%02x", $0) }.joined()
  }
  func proof(code: String, challenge: String, name: String) throws -> String {
    guard let challenge = Data(base64Encoded: challenge), challenge.count == 32 else {
      throw ClientFailure.invalidFrame
    }
    let derived = SHA256.hash(data: Data(code.utf8) + key.publicKey.rawRepresentation)
    let mac = HMAC<SHA256>.authenticationCode(
      for: challenge + Data(name.utf8), using: SymmetricKey(data: derived))
    return Data(mac).base64EncodedString()
  }
}
