import Foundation
import Security

/// Where a client keeps the API keys of its voice providers. A key is the device's own: it is
/// never sent to the engine, never written to a setting and never shown again after it is saved.
public protocol ProviderKeyVault: Sendable {
  /// The key saved for `provider`, or nil when there is none.
  func key(for provider: String) throws -> String?
  /// Saves `key` (trimmed of whitespace) for `provider`, replacing the one there. A key that is
  /// empty or has a space or a line break inside is refused: use `forget` to remove one.
  func save(_ key: String, for provider: String) throws
  func forget(provider: String) throws
}

public enum ProviderKeyFailure: Error, LocalizedError, Equatable, Sendable {
  case invalidKey
  case keychain(Int32)

  public var errorDescription: String? {
    switch self {
    case .invalidKey: "The key cannot be empty or contain blanks."
    case .keychain(let status): "Could not access the protected key (\(status))."
    }
  }
}

/// The system keychain. Items are `WhenUnlockedThisDeviceOnly`: they do not leave the device in
/// a backup. The service name carries "next" like every identifier of the rewrite.
public struct KeychainProviderKeyVault: ProviderKeyVault {
  public static let service = "com.soyeht.next.voice-provider-keys"

  private let service: String

  /// Tests that must not touch the person's keychain pass their own service name and still use
  /// a fake vault; only the app uses the default.
  public init(service: String = Self.service) {
    self.service = service
  }

  public func key(for provider: String) throws -> String? {
    let query = Self.query(service: service, provider: provider)
    guard let data = try KeychainItem.read(query, failure: ProviderKeyFailure.keychain) else {
      return nil
    }
    return String(data: data, encoding: .utf8)
  }

  public func save(_ key: String, for provider: String) throws {
    let trimmed = key.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !trimmed.isEmpty, trimmed.rangeOfCharacter(from: .whitespacesAndNewlines) == nil else {
      throw ProviderKeyFailure.invalidKey
    }
    let status = KeychainItem.upsert(
      Self.query(service: service, provider: provider), Self.attributes(of: trimmed))
    guard status == errSecSuccess else { throw ProviderKeyFailure.keychain(status) }
  }

  public func forget(provider: String) throws {
    let status = SecItemDelete(Self.query(service: service, provider: provider) as CFDictionary)
    guard status == errSecSuccess || status == errSecItemNotFound else {
      throw ProviderKeyFailure.keychain(status)
    }
  }

  /// What is stored with the key. `WhenUnlockedThisDeviceOnly`: the item does not leave the
  /// device in a backup, and cannot be read while the device is locked.
  static func attributes(of key: String) -> [String: Any] {
    [
      kSecValueData as String: Data(key.utf8),
      kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
    ]
  }

  static func query(service: String, provider: String) -> [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: provider,
    ]
  }
}
