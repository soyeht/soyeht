import CryptoKit
import Foundation
import SoyehtProtocol

/// `MAJOR.MINOR.PATCH`: three plain numbers, none with a leading zero and each below 1000.
public struct UpdateVersion: Comparable, Equatable, Sendable, CustomStringConvertible {
  private static let partLimit = 1000
  private let parts: [Int]

  public init?(_ text: String) {
    let pieces = text.split(separator: ".", omittingEmptySubsequences: false)
    guard pieces.count == 3 else { return nil }
    var numbers: [Int] = []
    for piece in pieces {
      let plain = !piece.isEmpty && piece.utf8.allSatisfy { (0x30...0x39).contains($0) }
      guard plain, piece == "0" || !piece.hasPrefix("0"), let number = Int(piece),
        number < Self.partLimit
      else { return nil }
      numbers.append(number)
    }
    parts = numbers
  }

  public var description: String { parts.map(String.init).joined(separator: ".") }

  public static func < (left: UpdateVersion, right: UpdateVersion) -> Bool {
    left.parts.lexicographicallyPrecedes(right.parts)
  }
}

/// What a build is: the version it runs and the package it would take.
public struct UpdateBuild: Equatable, Sendable {
  public let version: UpdateVersion
  /// As `std::env::consts::OS` spells it (`macos`).
  public let system: String
  /// As `std::env::consts::ARCH` spells it (`aarch64`, `x86_64`).
  public let architecture: String
  public let channel: String

  public init(version: UpdateVersion, system: String, architecture: String, channel: String) {
    self.version = version
    self.system = system
    self.architecture = architecture
    self.channel = channel
  }

  /// This build: the product version, the Mac and the stable channel. The iPhone and the iPad
  /// are updated by the App Store, so they have none.
  public static var current: UpdateBuild? {
    #if os(macOS)
      #if arch(arm64)
        let architecture = "aarch64"
      #else
        let architecture = "x86_64"
      #endif
      return UpdateVersion(ProductVersion.current).map {
        UpdateBuild(
          version: $0, system: "macos", architecture: architecture,
          channel: UpdatePolicy.defaultChannel)
      }
    #else
      return nil
    #endif
  }
}

/// The package of a newer version for this system and architecture.
public struct UpdatePackage: Equatable, Sendable, Decodable {
  public let url: String
  public let sha256: String
  public let size: UInt64
}

/// A newer version and where to get it.
public struct UpdateOffer: Equatable, Sendable {
  public let version: String
  /// The running version is below the minimum supported one.
  public let required: Bool
  public let notesURL: String
  public let package: UpdatePackage
}

public enum UpdateOutcome: Equatable, Sendable {
  case upToDate
  case available(UpdateOffer)
}

/// Reading the signed update manifest (ADR 0122, contract 15). `soyeht_protocol::update` and
/// `contracts/update_manifest.py` follow the same order of checks, and
/// `contracts/vectors/update.json` holds the cases all three pass.
public enum UpdateCheck {
  private struct Envelope: Decodable {
    let format: Int
    let keyId: String
    let payload: String
    let signature: String
  }

  private struct Listed: Decodable {
    let system: String
    let architecture: String
    let url: String
    let sha256: String
    let size: UInt64
  }

  private struct Payload: Decodable {
    let format: Int
    let channel: String
    let version: String
    let minimumSupportedVersion: String
    let releaseNotesUrl: String
    let packages: [Listed]
  }

  private static let addressLimit = 2048
  private static let sha256Length = 64

  /// What `build` should do with the manifest `text`, trusting `keys`.
  public static func evaluate(
    _ text: String, build: UpdateBuild, keys: [UpdatePublicKey] = UpdatePolicy.publicKeys
  ) -> Result<UpdateOutcome, UpdateFailure> {
    do {
      let payload = try checked(try open(text, keys: keys))
      guard payload.channel == build.channel else { return .failure(.wrongChannel) }
      guard let version = UpdateVersion(payload.version),
        let minimum = UpdateVersion(payload.minimumSupportedVersion)
      else { return .failure(.badManifest) }
      guard version > build.version else { return .success(.upToDate) }
      guard
        let listed = payload.packages.first(where: {
          $0.system == build.system && $0.architecture == build.architecture
        })
      else { return .failure(.noPackage) }
      return .success(
        .available(
          UpdateOffer(
            version: payload.version, required: build.version < minimum,
            notesURL: payload.releaseNotesUrl,
            package: UpdatePackage(url: listed.url, sha256: listed.sha256, size: listed.size))))
    } catch let failure as UpdateFailure {
      return .failure(failure)
    } catch {
      return .failure(.badManifest)
    }
  }

  /// `type` read from `data`; the failure is the caller's: a manifest that does not decode is
  /// `badEnvelope` or `badManifest`, which the check reports.
  private static func decoded<T: Decodable>(_ type: T.Type, _ data: Data) -> T? {
    do { return try JSONDecoder().decode(type, from: data) } catch { return nil }
  }

  /// The key of 32 raw bytes; anything else never verifies a signature.
  private static func signingKey(_ raw: Data) -> Curve25519.Signing.PublicKey? {
    do { return try Curve25519.Signing.PublicKey(rawRepresentation: raw) } catch { return nil }
  }

  /// The bytes of padded standard base64, and nothing that merely decodes to them.
  private static func canonicalBase64(_ text: String) -> Data? {
    guard let raw = Data(base64Encoded: text), raw.base64EncodedString() == text else {
      return nil
    }
    return raw
  }

  private static func isAddress(_ value: String) -> Bool {
    value.utf8.count <= addressLimit && value.utf8.allSatisfy { (0x21...0x7e).contains($0) }
      && UpdatePolicy.urlPrefixes.contains { value.hasPrefix($0) }
  }

  /// The payload bytes of `text` when a key of `keys` signed them.
  private static func open(_ text: String, keys: [UpdatePublicKey]) throws -> Data {
    guard !keys.isEmpty else { throw UpdateFailure.notConfigured }
    guard text.utf8.count <= UpdatePolicy.manifestMaxBytes else { throw UpdateFailure.tooLarge }
    guard let envelope = decoded(Envelope.self, Data(text.utf8)),
      envelope.format == UpdatePolicy.manifestFormat
    else { throw UpdateFailure.badEnvelope }
    guard let key = keys.first(where: { $0.id == envelope.keyId }) else {
      throw UpdateFailure.unknownKey
    }
    guard let payload = canonicalBase64(envelope.payload),
      let signature = canonicalBase64(envelope.signature), signature.count == 64
    else { throw UpdateFailure.badEnvelope }
    // A key in the contract that is not 32 bytes of base64 never verifies anything.
    guard let raw = canonicalBase64(key.key),
      let publicKey = signingKey(raw),
      publicKey.isValidSignature(signature, for: Data(UpdatePolicy.signingContext.utf8) + payload)
    else { throw UpdateFailure.badSignature }
    return payload
  }

  /// The payload when it keeps every rule of the format.
  private static func checked(_ bytes: Data) throws -> Payload {
    guard let payload = decoded(Payload.self, bytes),
      payload.format == UpdatePolicy.manifestFormat,
      UpdatePolicy.channels.contains(payload.channel),
      let version = UpdateVersion(payload.version),
      let minimum = UpdateVersion(payload.minimumSupportedVersion), minimum <= version,
      isAddress(payload.releaseNotesUrl),
      !payload.packages.isEmpty, payload.packages.count <= UpdatePolicy.maxPackages
    else { throw UpdateFailure.badManifest }
    var seen = Set<String>()
    for listed in payload.packages {
      guard seen.insert("\(listed.system) \(listed.architecture)").inserted,
        UpdatePolicy.systems.contains(listed.system),
        UpdatePolicy.architectures.contains(listed.architecture),
        isAddress(listed.url), isSha256(listed.sha256),
        listed.size > 0, listed.size <= UpdatePolicy.maxPackageBytes
      else { throw UpdateFailure.badManifest }
    }
    return payload
  }

  private static func isSha256(_ value: String) -> Bool {
    value.utf8.count == sha256Length
      && value.utf8.allSatisfy { (0x30...0x39).contains($0) || (0x61...0x66).contains($0) }
  }
}
