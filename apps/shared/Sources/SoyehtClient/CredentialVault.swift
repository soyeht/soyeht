import Foundation
import Security

public enum CredentialVault {
  private static var query: [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      // Kept on purpose, for every build: a different service name would hide the credential of
      // every device already paired and force each one to pair again.
      kSecAttrService as String: "com.soyeht.next.dev.ios.identity",
      kSecAttrAccount as String: "paired-engine",
    ]
  }
  public static func load() throws -> DeviceCredential? {
    guard let data = try KeychainItem.read(query, failure: failure) else { return nil }
    return try JSONDecoder().decode(DeviceCredential.self, from: data)
  }
  public static func save(_ credential: DeviceCredential) throws {
    let values: [String: Any] = [
      kSecValueData as String: try JSONEncoder().encode(credential),
      kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
    ]
    let status = KeychainItem.upsert(query, values)
    guard status == errSecSuccess else { throw failure(status) }
  }
  public static func forget() throws {
    let status = SecItemDelete(query as CFDictionary)
    guard status == errSecSuccess || status == errSecItemNotFound else { throw failure(status) }
  }
  private static func failure(_ status: OSStatus) -> ClientFailure {
    .remote("keychain_unavailable", "Could not access the protected credential (\(status)).")
  }
}
