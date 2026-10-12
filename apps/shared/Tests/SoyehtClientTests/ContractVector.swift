import Foundation
import SoyehtProtocol

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

  static func cases(_ vector: JsonValue, _ key: String) -> [JsonValue] {
    vector[key].array ?? []
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
