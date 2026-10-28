import CryptoKit
import Foundation
import Network
import SoyehtProtocol

public struct PairingLink: Sendable {
  public let engineId: String
  public let address: URL
  let code: String
  public init(_ text: String) throws {
    guard let parts = URLComponents(string: text), parts.scheme == "soyeht-next",
      parts.host == "pair", parts.path.isEmpty, parts.fragment == nil,
      let items = parts.queryItems, items.count == 3,
      Set(items.map(\.name)) == Set(["engineId", "address", "code"]),
      let engine = items.first(where: { $0.name == "engineId" })?.value,
      UUID(uuidString: engine) != nil,
      let code = items.first(where: { $0.name == "code" })?.value,
      code.utf8.count == 43,
      code.allSatisfy({ $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-" || $0 == "_") }),
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
    self.address = url
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

  init(engineId: String, deviceId: String, address: URL, token: String, key: Data) {
    self.engineId = engineId
    self.deviceId = deviceId
    self.address = address
    self.token = token
    self.key = key
  }
  func hello(challenge: AuthChallengeEvent) throws -> HelloParams {
    guard challenge.engineId == engineId, let nonce = Data(base64Encoded: challenge.nonce),
      nonce.count == 32
    else {
      throw ClientFailure.remote(
        ErrorKind.engine_identity_changed.rawValue, "The Mac identity changed. Pair again.")
    }
    let key = try Curve25519.Signing.PrivateKey(rawRepresentation: key)
    let bytes = Data("soyeht-hello-v1".utf8) + nonce + Data(engineId.utf8) + Data(token.utf8)
    return HelloParams(
      client: "iphone-dev", clientVersion: ProductVersion.current, protocolVersion: 1,
      deviceToken: token, proof: try key.signature(for: bytes).base64EncodedString())
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
