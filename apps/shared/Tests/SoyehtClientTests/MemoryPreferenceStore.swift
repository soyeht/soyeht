@testable import SoyehtClient

/// Client preferences kept in memory, shared by every preference test.
final class MemoryPreferenceStore: ClientPreferenceStore {
  var values: [String: Bool] = [:]

  func bool(forKey key: String) -> Bool { values[key] ?? false }
  func set(_ value: Bool, forKey key: String) { values[key] = value }
}
