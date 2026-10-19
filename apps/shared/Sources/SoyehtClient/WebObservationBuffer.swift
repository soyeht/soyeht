import Foundation
import SoyehtProtocol

public enum WebObservationError: Error, Equatable, Sendable {
  case invalidConsoleEntry
  case invalidNetworkEntry
  case invalidCursor
}

public struct WebObservationPage<Value: Sendable>: Sendable {
  public var values: [Value]
  public var next: UInt64
  public var truncated: Bool
}

private struct Sequenced<Value: Sendable>: Sendable {
  var sequence: UInt64
  var value: Value
}

public struct WebObservationBuffer: Sendable {
  private var console: [Sequenced<WebConsoleEntry>] = []
  private var network: [Sequenced<WebNetworkEntry>] = []
  private var nextConsoleSequence: UInt64 = 1
  private var nextNetworkSequence: UInt64 = 1

  public init() {}

  public var consoleCounts: (warnings: UInt32, errors: UInt32) {
    let warnings = console.lazy.filter { $0.value.level == .warning }.count
    let errors = console.lazy.filter { $0.value.level == .error }.count
    return (UInt32(warnings), UInt32(errors))
  }

  public mutating func appendConsole(
    level: WebConsoleLevel, text: String, source: String, at: String
  ) throws {
    guard text.utf8.count <= ProtocolLimits.webTextBytes,
      source.utf8.count <= ProtocolLimits.webFieldBytes, validTimestamp(at)
    else {
      throw WebObservationError.invalidConsoleEntry
    }
    let entry = WebConsoleEntry(
      sequence: String(nextConsoleSequence), level: level, text: text, source: source, at: at)
    if console.count == ProtocolLimits.webConsoleEntries { console.removeFirst() }
    console.append(Sequenced(sequence: nextConsoleSequence, value: entry))
    if nextConsoleSequence < UInt64.max { nextConsoleSequence += 1 }
  }

  public mutating func appendNetwork(
    method: String, url: String, status: UInt32, durationMs: UInt32, bytes: UInt64, at: String
  ) throws {
    guard !method.isEmpty, method.utf8.count <= 32, !url.isEmpty,
      url.utf8.count <= ProtocolLimits.webFieldBytes,
      status <= 999, validTimestamp(at)
    else { throw WebObservationError.invalidNetworkEntry }
    let entry = WebNetworkEntry(
      sequence: String(nextNetworkSequence), method: method, url: url, status: status,
      durationMs: durationMs, bytes: String(bytes), at: at)
    if network.count == ProtocolLimits.webNetworkEntries { network.removeFirst() }
    network.append(Sequenced(sequence: nextNetworkSequence, value: entry))
    if nextNetworkSequence < UInt64.max { nextNetworkSequence += 1 }
  }

  public func consolePage(since: String?) throws -> WebObservationPage<WebConsoleEntry> {
    try page(console, since: since)
  }

  public func networkPage(since: String?) throws -> WebObservationPage<WebNetworkEntry> {
    try page(network, since: since)
  }

  private func page<Value: Encodable & Sendable>(
    _ ring: [Sequenced<Value>], since rawSince: String?
  ) throws -> WebObservationPage<Value> {
    let requested: UInt64
    if let rawSince {
      guard let parsed = UInt64(rawSince) else { throw WebObservationError.invalidCursor }
      requested = parsed
    } else {
      requested = 0
    }
    let first = ring.first?.sequence ?? saturatedAdd(requested, 1)
    var result: [Value] = []
    var bytes = 2
    var next = requested
    var truncated = saturatedAdd(requested, 1) < first
    for entry in ring where entry.sequence > requested {
      let count = try JSONEncoder().encode(entry.value).count + (result.isEmpty ? 0 : 1)
      if bytes + count > ProtocolLimits.jsonBytes - 4_096 {
        truncated = true
        break
      }
      result.append(entry.value)
      bytes += count
      next = entry.sequence
    }
    return WebObservationPage(values: result, next: next, truncated: truncated)
  }

  private func validTimestamp(_ value: String) -> Bool {
    !value.isEmpty && value.utf8.count <= 64
  }

  private func saturatedAdd(_ lhs: UInt64, _ rhs: UInt64) -> UInt64 {
    let (sum, overflow) = lhs.addingReportingOverflow(rhs)
    return overflow ? UInt64.max : sum
  }
}
