import Foundation
import Security

enum KeychainItem {
  /// The data of the item that `query` names, or nil when there is none. Any other failure is
  /// reported through `failure`.
  static func read(_ query: [String: Any], failure: (OSStatus) -> any Error) throws -> Data? {
    var request = query
    request[kSecReturnData as String] = true
    request[kSecMatchLimit as String] = kSecMatchLimitOne
    var result: CFTypeRef?
    let status = SecItemCopyMatching(request as CFDictionary, &result)
    if status == errSecItemNotFound { return nil }
    guard status == errSecSuccess, let data = result as? Data else { throw failure(status) }
    return data
  }

  /// Updates the item that `query` names, or adds it when there is none. Returns
  /// `errSecSuccess`, or the status that failed.
  static func upsert(_ query: [String: Any], _ values: [String: Any]) -> OSStatus {
    let status = SecItemUpdate(query as CFDictionary, values as CFDictionary)
    guard status == errSecItemNotFound else { return status }
    return SecItemAdd(query.merging(values) { _, value in value } as CFDictionary, nil)
  }
}
