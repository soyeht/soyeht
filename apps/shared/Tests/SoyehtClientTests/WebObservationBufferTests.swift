import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private let observedAt = "2026-09-13T12:00:00Z"

@Test func observationRingsAreBoundedAndReportExclusiveCursorLoss() throws {
  var buffer = WebObservationBuffer()
  for index in 0...ProtocolLimits.webConsoleEntries {
    try buffer.appendConsole(
      level: index.isMultiple(of: 2) ? .warning : .error, text: "entry \(index)",
      source: "https://example.invalid", at: observedAt)
  }
  let page = try buffer.consolePage(since: "0")

  #expect(page.values.count == ProtocolLimits.webConsoleEntries)
  #expect(page.values.first?.sequence == "2")
  #expect(page.next == UInt64(ProtocolLimits.webConsoleEntries + 1))
  #expect(page.truncated)
  #expect(buffer.consoleCounts.warnings == 250)
  #expect(buffer.consoleCounts.errors == 250)
}

@Test func observationPagesAreByteBoundedAndRejectInvalidMetadata() throws {
  var buffer = WebObservationBuffer()
  for _ in 0..<ProtocolLimits.webConsoleEntries {
    try buffer.appendConsole(
      level: .info, text: String(repeating: "x", count: 16_384), source: "page",
      at: observedAt)
  }
  let page = try buffer.consolePage(since: nil)
  let bytes = try JSONEncoder().encode(page.values).count

  #expect(bytes <= ProtocolLimits.jsonBytes - 4_096)
  #expect(page.truncated)
  #expect(throws: WebObservationError.invalidCursor) {
    _ = try buffer.consolePage(since: "old")
  }
  #expect(throws: WebObservationError.invalidNetworkEntry) {
    try buffer.appendNetwork(
      method: "GET", url: "", status: 200, durationMs: 1, bytes: 0, at: observedAt)
  }
}
