import Foundation
import SoyehtProtocol

func codec<T: Codable>(_ type: T.Type, _ data: Data) throws -> Data {
    try JSONEncoder().encode(JSONDecoder().decode(type, from: data))
}

struct Vector: Decodable {
    var name: String
    var type: String
    var value: JsonValue
}

let path = CommandLine.arguments.dropFirst().first ?? "../vectors/messages.json"
let vectors = try JSONDecoder().decode([Vector].self, from: Data(contentsOf: URL(fileURLWithPath: path)))
for vector in vectors {
    let encoded = try roundtrip(vector.type, JSONEncoder().encode(vector.value))
    let actual = try JSONDecoder().decode(JsonValue.self, from: encoded)
    guard actual == vector.value else {
        fatalError("Round-trip mismatch: \(vector.name)")
    }
}
print("Swift: \(vectors.count) typed message vectors passed")

struct InvalidVector: Decodable {
    var type: String
    var value: JsonValue
}
let invalidURL = URL(fileURLWithPath: path).deletingLastPathComponent().appendingPathComponent("invalid-messages.json")
let invalidVectors = try JSONDecoder().decode([InvalidVector].self, from: Data(contentsOf: invalidURL))
for vector in invalidVectors {
    let data = try JSONEncoder().encode(vector.value)
    precondition((try? roundtrip(vector.type, data)) == nil, "Invalid value accepted: \(vector.type)")
}

struct FrameVector: Decodable {
    var name: String
    var hex: String
    var kind: UInt8
    var payloadHex: String
    var headerHex: String?
    var paneRef: UInt32?
    var generation: UInt32?
    var offset: String?
}

func unhex(_ text: String) -> Data {
    let chars = Array(text)
    return Data(stride(from: 0, to: chars.count, by: 2).map { UInt8(String(chars[$0...($0 + 1)]), radix: 16)! })
}

let framesURL = URL(fileURLWithPath: path).deletingLastPathComponent().appendingPathComponent("frames.json")
let frames = try JSONDecoder().decode([FrameVector].self, from: Data(contentsOf: framesURL))
for vector in frames {
    let wire = unhex(vector.hex)
    let frame = try Frame.decode(wire)
    precondition(frame.kind == vector.kind && frame.payload == unhex(vector.payloadHex))
    if let headerHex = vector.headerHex { precondition(frame.header == unhex(headerHex)) }
    if frame.kind == 2 {
        let bytes = Array(frame.header)
        precondition(bytes[0..<4].reduce(UInt32(0)) { ($0 << 8) | UInt32($1) } == vector.paneRef)
        precondition(bytes[4..<8].reduce(UInt32(0)) { ($0 << 8) | UInt32($1) } == vector.generation)
        precondition(String(bytes[8..<16].reduce(UInt64(0)) { ($0 << 8) | UInt64($1) }) == vector.offset)
    }
    let encoded = try frame.encode()
    precondition(encoded == wire)
}
for bad in ["02ffffffff", "020000000f", "01000000027b", "0000000001ff", "01000000027b7d00"] {
    precondition((try? Frame.decode(unhex(bad))) == nil)
}
print("Swift: \(frames.count) binary vectors and rejection controls passed")
