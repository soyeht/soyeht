import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func snapshotCommitsOnlyCompleteMatchingParts() throws {
  var assembly = SnapshotAssembly()
  try assembly.append(
    StateSnapshotPartEvent(snapshotId: "s", index: 0, of: 2, section: .panes, data: .array([])))
  #expect(throws: ClientFailure.self) {
    try assembly.finish(StateSnapshotResult(snapshotId: "s", cursor: "3", parts: 2))
  }
  #expect(throws: ClientFailure.self) {
    try assembly.append(
      StateSnapshotPartEvent(
        snapshotId: "other", index: 1, of: 2, section: .workspaces, data: .array([])))
  }
  try assembly.append(
    StateSnapshotPartEvent(snapshotId: "s", index: 1, of: 2, section: .workspaces, data: .array([]))
  )
  #expect(
    try assembly.finish(StateSnapshotResult(snapshotId: "s", cursor: "3", parts: 2)).count == 2)
}

private func terminal(reference: UInt32 = 2, generation: UInt32 = 1, offset: UInt64, bytes: Data)
  -> Frame
{
  var header = Data()
  for value in [reference, generation] {
    for shift in stride(from: 24, through: 0, by: -8) {
      header.append(UInt8(truncatingIfNeeded: value >> shift))
    }
  }
  for shift in stride(from: 56, through: 0, by: -8) {
    header.append(UInt8(truncatingIfNeeded: offset >> shift))
  }
  return Frame(kind: 2, header: header, payload: bytes)
}

@Test func terminalRejectsSilentHolesAndOldGenerations() throws {
  var stream = StreamPosition(streamId: "s", reference: 2, generation: 1)
  #expect(
    try stream.start(
      StreamReplayStartEvent(
        streamId: "s", generation: 1, fromOffset: "10", toOffset: "20", reset: true)))
  #expect(try stream.accept(terminal(reference: 1, offset: 10, bytes: Data([1]))) == nil)
  #expect(throws: ClientFailure.self) { try stream.accept(terminal(offset: 11, bytes: Data([1]))) }
  #expect(throws: ClientFailure.self) {
    try stream.accept(terminal(generation: 2, offset: 10, bytes: Data([1])))
  }
  #expect(try stream.accept(terminal(offset: 10, bytes: Data([27, 91]))) == Data([27, 91]))
  #expect(stream.next == 12)
  try stream.gap(StreamGapEvent(streamId: "s", fromOffset: "12", toOffset: "19"))
  #expect(try stream.accept(terminal(offset: 19, bytes: Data([65]))) == Data([65]))
  #expect(
    try !stream.start(
      StreamReplayStartEvent(
        streamId: "s", generation: 1, fromOffset: "20", toOffset: "20", reset: false)))
  #expect(throws: ClientFailure.self) {
    try stream.start(
      StreamReplayStartEvent(
        streamId: "s", generation: 1, fromOffset: "19", toOffset: "20", reset: false))
  }
}

@Test func fixtureSocketCannotSelectAnInstalledProfile() {
  #expect(
    DevProfile.fixtureSocket("/tmp/sn-ui-123/engine.sock") == "/tmp/sn-ui-123/engine.sock")
  #expect(
    DevProfile.fixtureSocket(DevProfile.dataDirectory.appendingPathComponent("engine.sock").path)
      == nil)
  #expect(DevProfile.fixtureSocket("/tmp/engine.sock") == nil)
  #expect(DevProfile.fixtureSocket("/tmp/sn-ui-123/../other/engine.sock") == nil)
}
