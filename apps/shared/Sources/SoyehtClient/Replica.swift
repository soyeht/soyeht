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
  mutating func gap(_ event: StreamGapEvent) throws -> Bool {
    guard event.streamId == streamId, let from = UInt64(event.fromOffset),
      let to = UInt64(event.toOffset), next == from, to >= from
    else { throw ClientFailure.invalidStream }
    next = to
    return to > from
  }
}

struct TerminalStreamDelivery: Equatable {
  var paneId: String
  var bytes: Data
}

struct TerminalStreamReset: Equatable {
  var paneId: String
  var reset: Bool
}

/// Keeps every terminal stream independent on one engine connection.
///
/// Binary frames identify a subscription by its connection-local reference, while replay
/// notifications identify it by stream id. Both indexes point back to the pane that owns the
/// position, so interleaved output can never be delivered through another pane's sink.
struct TerminalStreamReplica {
  private var positions: [String: StreamPosition] = [:]
  private var paneByReference: [UInt32: String] = [:]
  private var paneByStream: [String: String] = [:]
  private(set) var gapTextByPane: [String: String] = [:]

  var count: Int { positions.count }

  func contains(_ paneId: String) -> Bool { positions[paneId] != nil }

  func gapText(for paneId: String) -> String? { gapTextByPane[paneId] }

  mutating func install(paneId: String, info: StreamInfo) throws {
    guard info.paneId == paneId,
      paneByReference[info.binaryPaneRef].map({ $0 == paneId }) ?? true,
      paneByStream[info.streamId].map({ $0 == paneId }) ?? true
    else { throw ClientFailure.invalidStream }
    remove(paneId)
    positions[paneId] = StreamPosition(
      streamId: info.streamId, reference: info.binaryPaneRef,
      generation: info.generation, next: nil)
    paneByReference[info.binaryPaneRef] = paneId
    paneByStream[info.streamId] = paneId
  }

  mutating func accept(_ frame: Frame) throws -> TerminalStreamDelivery? {
    let reference = try Self.reference(in: frame)
    guard let paneId = paneByReference[reference], var position = positions[paneId] else {
      return nil
    }
    let bytes = try position.accept(frame)
    positions[paneId] = position
    return bytes.map { TerminalStreamDelivery(paneId: paneId, bytes: $0) }
  }

  mutating func start(_ event: StreamReplayStartEvent) throws -> TerminalStreamReset? {
    guard let paneId = paneByStream[event.streamId], var position = positions[paneId] else {
      return nil
    }
    let reset = try position.start(event)
    positions[paneId] = position
    if reset { gapTextByPane.removeValue(forKey: paneId) }
    return TerminalStreamReset(paneId: paneId, reset: reset)
  }

  mutating func gap(_ event: StreamGapEvent) throws -> String? {
    guard let paneId = paneByStream[event.streamId], var position = positions[paneId] else {
      return nil
    }
    let omittedBytes = try position.gap(event)
    positions[paneId] = position
    guard omittedBytes else { return nil }
    gapTextByPane[paneId] = TerminalStatePresentation.recordedGap.text
    return paneId
  }

  mutating func finish(_ event: StreamReplayDoneEvent) throws -> String? {
    guard let paneId = paneByStream[event.streamId], let position = positions[paneId] else {
      return nil
    }
    guard event.nextOffset == position.next.map(String.init) else {
      throw ClientFailure.invalidStream
    }
    return paneId
  }

  mutating func remove(_ paneId: String) {
    guard let position = positions.removeValue(forKey: paneId) else {
      gapTextByPane.removeValue(forKey: paneId)
      return
    }
    if paneByReference[position.reference] == paneId {
      paneByReference.removeValue(forKey: position.reference)
    }
    if paneByStream[position.streamId] == paneId {
      paneByStream.removeValue(forKey: position.streamId)
    }
    gapTextByPane.removeValue(forKey: paneId)
  }

  mutating func retainOnly(_ paneIds: Set<String>) {
    for paneId in positions.keys where !paneIds.contains(paneId) { remove(paneId) }
    gapTextByPane = gapTextByPane.filter { paneIds.contains($0.key) }
  }

  mutating func removeAll() {
    positions.removeAll()
    paneByReference.removeAll()
    paneByStream.removeAll()
    gapTextByPane.removeAll()
  }

  private static func reference(in frame: Frame) throws -> UInt32 {
    let header = Array(frame.header)
    guard frame.kind == 2, header.count == 16 else { throw ClientFailure.invalidFrame }
    return header[0..<4].reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
  }
}
