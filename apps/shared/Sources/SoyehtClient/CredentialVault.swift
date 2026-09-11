import Foundation
import Security

public enum CredentialVault {
  private static var query: [String: Any] {
    [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: "com.soyeht.next.dev.ios.identity",
      kSecAttrAccount as String: "paired-engine",
    ]
  }
  public static func load() throws -> DeviceCredential? {
    var request = query
    request[kSecReturnData as String] = true
    request[kSecMatchLimit as String] = kSecMatchLimitOne
    var result: CFTypeRef?
    let status = SecItemCopyMatching(request as CFDictionary, &result)
    if status == errSecItemNotFound { return nil }
    guard status == errSecSuccess, let data = result as? Data else { throw failure(status) }
    return try JSONDecoder().decode(DeviceCredential.self, from: data)
  }
  public static func save(_ credential: DeviceCredential) throws {
    let values: [String: Any] = [
      kSecValueData as String: try JSONEncoder().encode(credential),
      kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
    ]
    let status = SecItemUpdate(query as CFDictionary, values as CFDictionary)
    if status == errSecItemNotFound {
      let insertion = query.merging(values) { _, value in value }
      let status = SecItemAdd(insertion as CFDictionary, nil)
      guard status == errSecSuccess else { throw failure(status) }
    } else if status != errSecSuccess {
      throw failure(status)
    }
  }
  public static func forget() throws {
    let status = SecItemDelete(query as CFDictionary)
    guard status == errSecSuccess || status == errSecItemNotFound else { throw failure(status) }
  }
  private static func failure(_ status: OSStatus) -> ClientFailure {
    .remote("keychain_unavailable", "Não foi possível acessar a credencial protegida (\(status)).")
  }
}
