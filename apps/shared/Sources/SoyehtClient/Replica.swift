import Foundation
import SoyehtProtocol

struct SnapshotAssembly {
  private var identity: String?
  private var count: UInt32?
  private var parts: [StateSnapshotPartEvent] = []
  mutating func append(_ part: StateSnapshotPartEvent) throws {
    guard part.index == parts.count, part.of > 0,
      part.index < part.of, part.data.array != nil,
      identity == nil || identity == part.snapshotId,
      count == nil || count == part.of
    else { throw ClientFailure.invalidSnapshot }
    identity = part.snapshotId
    count = part.of
    parts.append(part)
  }
  func finish(_ result: StateSnapshotResult) throws -> [String: [JsonValue]] {
    guard identity == result.snapshotId, count == result.parts,
      parts.count == result.parts, UInt64(result.cursor) != nil
    else {
      throw ClientFailure.invalidSnapshot
    }
    var sections: [String: [JsonValue]] = [:]
    for part in parts { sections[part.section.rawValue, default: []] += part.data.array ?? [] }
    return sections
  }
}

struct StreamPosition {
  var streamId: String
  var reference: UInt32
  var generation: UInt32
  var next: UInt64?
  mutating func start(_ event: StreamReplayStartEvent) throws -> Bool {
    guard event.streamId == streamId, event.generation == generation,
      let from = UInt64(event.fromOffset), let to = UInt64(event.toOffset), from <= to,
      event.reset || next == from
    else { throw ClientFailure.invalidStream }
    next = from
    return event.reset
  }
  mutating func accept(_ frame: Frame) throws -> Data? {
    let header = Array(frame.header)
    guard frame.kind == 2, header.count == 16 else { throw ClientFailure.invalidFrame }
    let reference = header[0..<4].reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
    let generation = header[4..<8].reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
    let offset = header[8..<16].reduce(UInt64(0)) { ($0 << 8) | UInt64($1) }
    guard reference == self.reference else { return nil }
    guard generation == self.generation, next == offset,
      offset <= UInt64.max - UInt64(frame.payload.count)
    else { throw ClientFailure.invalidStream }
    next = offset + UInt64(frame.payload.count)
    return frame.payload
  }
  mutating func gap(_ event: StreamGapEvent) throws {
    guard event.streamId == streamId, let from = UInt64(event.fromOffset),
      let to = UInt64(event.toOffset), next == from, to >= from
    else { throw ClientFailure.invalidStream }
    next = to
  }
}
