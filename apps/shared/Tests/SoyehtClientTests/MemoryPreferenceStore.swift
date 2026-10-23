@testable import SoyehtClient

/// Client preferences kept in memory, shared by every preference test.
final class MemoryPreferenceStore: ClientPreferenceStore {
  var values: [String: Bool] = [:]
  var integers: [String: Int] = [:]

  func bool(forKey key: String) -> Bool { values[key] ?? false }
  func storedBool(forKey key: String) -> Bool? { values[key] }
  func set(_ value: Bool, forKey key: String) { values[key] = value }
  func integer(forKey key: String) -> Int? { integers[key] }
  func set(_ value: Int, forKey key: String) { integers[key] = value }
}
