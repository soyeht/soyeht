import Foundation

extension JsonValue {
  /// A typed value read straight from this tree. A frame is parsed to a tree once; a handler
  /// that wants a type used to write the tree out as text and parse that text again. It reads
  /// what `JSONDecoder` reads (`ProtocolVectors` runs every vector through both).
  public func decoded<T: Decodable>(_ type: T.Type) throws -> T {
    try JsonTreeDecoder.decode(type, from: self)
  }
}

struct JsonTreeDecoder: Decoder {
  let value: JsonValue
  var codingPath: [any CodingKey] = []
  var userInfo: [CodingUserInfoKey: Any] { [:] }

  static func decode<T: Decodable>(_ type: T.Type, from value: JsonValue) throws -> T {
    try decode(type, from: value, at: [])
  }

  fileprivate static func decode<T: Decodable>(
    _ type: T.Type, from value: JsonValue, at path: [any CodingKey]
  ) throws -> T {
    if type == JsonValue.self, let same = value as? T { return same }
    // The types `JSONDecoder` reads in a way of their own, with its default strategies.
    if type == Data.self, case .string(let text) = value, let data = Data(base64Encoded: text),
      let same = data as? T
    {
      return same
    }
    if type == URL.self, case .string(let text) = value, let url = URL(string: text),
      let same = url as? T
    {
      return same
    }
    return try T(from: JsonTreeDecoder(value: value, codingPath: path))
  }

  func container<Key: CodingKey>(keyedBy type: Key.Type) throws -> KeyedDecodingContainer<Key> {
    guard case .object(let object) = value else { throw mismatch([String: JsonValue].self) }
    return KeyedDecodingContainer(KeyedTree<Key>(object: object, codingPath: codingPath))
  }

  func unkeyedContainer() throws -> any UnkeyedDecodingContainer {
    guard case .array(let items) = value else { throw mismatch([JsonValue].self) }
    return UnkeyedTree(items: items, codingPath: codingPath)
  }

  func singleValueContainer() throws -> any SingleValueDecodingContainer {
    SingleTree(value: value, codingPath: codingPath)
  }

  private func mismatch(_ type: Any.Type) -> DecodingError {
    value.mismatch(type, at: codingPath)
  }
}

extension JsonValue {
  fileprivate func mismatch(_ type: Any.Type, at path: [any CodingKey]) -> DecodingError {
    .typeMismatch(type, .init(codingPath: path, debugDescription: "Expected \(type)"))
  }

  fileprivate func bool(at path: [any CodingKey]) throws -> Bool {
    guard case .bool(let value) = self else { throw mismatch(Bool.self, at: path) }
    return value
  }

  fileprivate func string(at path: [any CodingKey]) throws -> String {
    guard case .string(let value) = self else { throw mismatch(String.self, at: path) }
    return value
  }

  fileprivate func double(at path: [any CodingKey]) throws -> Double {
    switch self {
    case .integer(let value): return Double(value)
    case .number(let value): return value
    default: throw mismatch(Double.self, at: path)
    }
  }

  /// A whole number, written as an integer or as a number with no fraction.
  fileprivate func integer<T: BinaryInteger>(
    _ type: T.Type, at path: [any CodingKey]
  ) throws -> T {
    let exact: T?
    switch self {
    case .integer(let value): exact = T(exactly: value)
    case .number(let value): exact = T(exactly: value)
    default: exact = nil
    }
    guard let exact else { throw mismatch(type, at: path) }
    return exact
  }
}

private struct SingleTree: SingleValueDecodingContainer {
  let value: JsonValue
  let codingPath: [any CodingKey]

  func decodeNil() -> Bool { value == .null }
  func decode(_ type: Bool.Type) throws -> Bool { try value.bool(at: codingPath) }
  func decode(_ type: String.Type) throws -> String { try value.string(at: codingPath) }
  func decode(_ type: Double.Type) throws -> Double { try value.double(at: codingPath) }
  func decode(_ type: Float.Type) throws -> Float { Float(try value.double(at: codingPath)) }
  func decode(_ type: Int.Type) throws -> Int { try value.integer(type, at: codingPath) }
  func decode(_ type: Int8.Type) throws -> Int8 { try value.integer(type, at: codingPath) }
  func decode(_ type: Int16.Type) throws -> Int16 { try value.integer(type, at: codingPath) }
  func decode(_ type: Int32.Type) throws -> Int32 { try value.integer(type, at: codingPath) }
  func decode(_ type: Int64.Type) throws -> Int64 { try value.integer(type, at: codingPath) }
  func decode(_ type: UInt.Type) throws -> UInt { try value.integer(type, at: codingPath) }
  func decode(_ type: UInt8.Type) throws -> UInt8 { try value.integer(type, at: codingPath) }
  func decode(_ type: UInt16.Type) throws -> UInt16 { try value.integer(type, at: codingPath) }
  func decode(_ type: UInt32.Type) throws -> UInt32 { try value.integer(type, at: codingPath) }
  func decode(_ type: UInt64.Type) throws -> UInt64 { try value.integer(type, at: codingPath) }
  func decode<T: Decodable>(_ type: T.Type) throws -> T {
    try JsonTreeDecoder.decode(type, from: value, at: codingPath)
  }
}

private struct IndexKey: CodingKey {
  let intValue: Int?
  var stringValue: String { "Index \(intValue ?? 0)" }
  init(_ index: Int) { intValue = index }
  init?(stringValue: String) { nil }
  init?(intValue: Int) { self.intValue = intValue }
}

private struct UnkeyedTree: UnkeyedDecodingContainer {
  let items: [JsonValue]
  let codingPath: [any CodingKey]
  private(set) var currentIndex = 0

  init(items: [JsonValue], codingPath: [any CodingKey]) {
    self.items = items
    self.codingPath = codingPath
  }

  var count: Int? { items.count }
  var isAtEnd: Bool { currentIndex >= items.count }

  private var pastEnd: DecodingError {
    .valueNotFound(
      Any.self, .init(codingPath: codingPath, debugDescription: "Past the end of the array"))
  }

  private mutating func next() throws -> SingleTree {
    guard !isAtEnd else { throw pastEnd }
    defer { currentIndex += 1 }
    return SingleTree(value: items[currentIndex], codingPath: codingPath + [IndexKey(currentIndex)])
  }

  mutating func decodeNil() throws -> Bool {
    guard !isAtEnd else { throw pastEnd }
    guard items[currentIndex] == .null else { return false }
    currentIndex += 1
    return true
  }
  mutating func decode(_ type: Bool.Type) throws -> Bool { try next().decode(type) }
  mutating func decode(_ type: String.Type) throws -> String { try next().decode(type) }
  mutating func decode(_ type: Double.Type) throws -> Double { try next().decode(type) }
  mutating func decode(_ type: Float.Type) throws -> Float { try next().decode(type) }
  mutating func decode(_ type: Int.Type) throws -> Int { try next().decode(type) }
  mutating func decode(_ type: Int8.Type) throws -> Int8 { try next().decode(type) }
  mutating func decode(_ type: Int16.Type) throws -> Int16 { try next().decode(type) }
  mutating func decode(_ type: Int32.Type) throws -> Int32 { try next().decode(type) }
  mutating func decode(_ type: Int64.Type) throws -> Int64 { try next().decode(type) }
  mutating func decode(_ type: UInt.Type) throws -> UInt { try next().decode(type) }
  mutating func decode(_ type: UInt8.Type) throws -> UInt8 { try next().decode(type) }
  mutating func decode(_ type: UInt16.Type) throws -> UInt16 { try next().decode(type) }
  mutating func decode(_ type: UInt32.Type) throws -> UInt32 { try next().decode(type) }
  mutating func decode(_ type: UInt64.Type) throws -> UInt64 { try next().decode(type) }
  mutating func decode<T: Decodable>(_ type: T.Type) throws -> T { try next().decode(type) }

  mutating func nestedContainer<NestedKey: CodingKey>(keyedBy type: NestedKey.Type) throws
    -> KeyedDecodingContainer<NestedKey>
  {
    let single = try next()
    return try JsonTreeDecoder(value: single.value, codingPath: single.codingPath)
      .container(keyedBy: type)
  }

  mutating func nestedUnkeyedContainer() throws -> any UnkeyedDecodingContainer {
    let single = try next()
    return try JsonTreeDecoder(value: single.value, codingPath: single.codingPath)
      .unkeyedContainer()
  }

  mutating func superDecoder() throws -> any Decoder {
    let single = try next()
    return JsonTreeDecoder(value: single.value, codingPath: single.codingPath)
  }
}

private struct KeyedTree<Key: CodingKey>: KeyedDecodingContainerProtocol {
  let object: [String: JsonValue]
  let codingPath: [any CodingKey]

  var allKeys: [Key] { object.keys.compactMap { Key(stringValue: $0) } }
  func contains(_ key: Key) -> Bool { object[key.stringValue] != nil }

  private func single(_ key: any CodingKey) throws -> SingleTree {
    guard let value = object[key.stringValue] else {
      throw DecodingError.keyNotFound(
        key, .init(codingPath: codingPath, debugDescription: "No value for \(key.stringValue)"))
    }
    return SingleTree(value: value, codingPath: codingPath + [key])
  }

  func decodeNil(forKey key: Key) throws -> Bool { try single(key).decodeNil() }
  func decode(_ type: Bool.Type, forKey key: Key) throws -> Bool { try single(key).decode(type) }
  func decode(_ type: String.Type, forKey key: Key) throws -> String {
    try single(key).decode(type)
  }
  func decode(_ type: Double.Type, forKey key: Key) throws -> Double {
    try single(key).decode(type)
  }
  func decode(_ type: Float.Type, forKey key: Key) throws -> Float { try single(key).decode(type) }
  func decode(_ type: Int.Type, forKey key: Key) throws -> Int { try single(key).decode(type) }
  func decode(_ type: Int8.Type, forKey key: Key) throws -> Int8 { try single(key).decode(type) }
  func decode(_ type: Int16.Type, forKey key: Key) throws -> Int16 { try single(key).decode(type) }
  func decode(_ type: Int32.Type, forKey key: Key) throws -> Int32 { try single(key).decode(type) }
  func decode(_ type: Int64.Type, forKey key: Key) throws -> Int64 { try single(key).decode(type) }
  func decode(_ type: UInt.Type, forKey key: Key) throws -> UInt { try single(key).decode(type) }
  func decode(_ type: UInt8.Type, forKey key: Key) throws -> UInt8 { try single(key).decode(type) }
  func decode(_ type: UInt16.Type, forKey key: Key) throws -> UInt16 {
    try single(key).decode(type)
  }
  func decode(_ type: UInt32.Type, forKey key: Key) throws -> UInt32 {
    try single(key).decode(type)
  }
  func decode(_ type: UInt64.Type, forKey key: Key) throws -> UInt64 {
    try single(key).decode(type)
  }
  func decode<T: Decodable>(_ type: T.Type, forKey key: Key) throws -> T {
    try single(key).decode(type)
  }

  func nestedContainer<NestedKey: CodingKey>(
    keyedBy type: NestedKey.Type, forKey key: Key
  ) throws -> KeyedDecodingContainer<NestedKey> {
    let single = try single(key)
    return try JsonTreeDecoder(value: single.value, codingPath: single.codingPath)
      .container(keyedBy: type)
  }

  func nestedUnkeyedContainer(forKey key: Key) throws -> any UnkeyedDecodingContainer {
    let single = try single(key)
    return try JsonTreeDecoder(value: single.value, codingPath: single.codingPath)
      .unkeyedContainer()
  }

  func superDecoder() throws -> any Decoder {
    JsonTreeDecoder(value: .object(object), codingPath: codingPath)
  }

  func superDecoder(forKey key: Key) throws -> any Decoder {
    let single = try single(key)
    return JsonTreeDecoder(value: single.value, codingPath: single.codingPath)
  }
}
