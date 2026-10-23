import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// A file of `contracts/vectors`: the cases the Mac client and the desktop client both pass.
enum ContractVector {
  static func load(_ name: String, file: String = #filePath) throws -> JsonValue {
    let root = URL(fileURLWithPath: file)
      .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
      .deletingLastPathComponent().deletingLastPathComponent()
    let data = try Data(contentsOf: root.appendingPathComponent("contracts/vectors/\(name).json"))
    return try JSONDecoder().decode(JsonValue.self, from: data)
  }

  /// The cases under `key`. A missing or empty section fails the test, so a misspelled key
  /// cannot make a vector pass without checking anything.
  static func cases(
    _ vector: JsonValue, _ key: String, sourceLocation: SourceLocation = #_sourceLocation
  ) -> [JsonValue] {
    guard let cases = vector[key].array, !cases.isEmpty else {
      Issue.record("The vector has no cases under \"\(key)\"", sourceLocation: sourceLocation)
      return []
    }
    return cases
  }
}

extension JsonValue {
  var bool: Bool? { if case .bool(let value) = self { value } else { nil } }
  var double: Double? {
    switch self {
    case .integer(let value): Double(value)
    case .number(let value): value
    default: nil
    }
  }
}
