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
  #expect(try stream.gap(StreamGapEvent(streamId: "s", fromOffset: "12", toOffset: "19")))
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

@Test func terminalStreamsRouteInterleavedFramesWithoutCrossFeed() throws {
  var streams = TerminalStreamReplica()
  try streams.install(
    paneId: "pane-a",
    info: StreamInfo(
      streamId: "stream-a", paneId: "pane-a", generation: 3,
      firstRetainedOffset: "0", nextOffset: "0", binaryPaneRef: 21))
  try streams.install(
    paneId: "pane-b",
    info: StreamInfo(
      streamId: "stream-b", paneId: "pane-b", generation: 8,
      firstRetainedOffset: "0", nextOffset: "0", binaryPaneRef: 34))

  #expect(
    try streams.start(
      StreamReplayStartEvent(
        streamId: "stream-a", generation: 3, fromOffset: "10", toOffset: "12",
        reset: true)) == TerminalStreamReset(paneId: "pane-a", reset: true))
  #expect(
    try streams.start(
      StreamReplayStartEvent(
        streamId: "stream-b", generation: 8, fromOffset: "40", toOffset: "42",
        reset: true)) == TerminalStreamReset(paneId: "pane-b", reset: true))

  #expect(
    try streams.accept(
      terminal(reference: 34, generation: 8, offset: 40, bytes: Data("B1".utf8)))
      == TerminalStreamDelivery(paneId: "pane-b", bytes: Data("B1".utf8)))
  #expect(
    try streams.accept(
      terminal(reference: 21, generation: 3, offset: 10, bytes: Data("A1".utf8)))
      == TerminalStreamDelivery(paneId: "pane-a", bytes: Data("A1".utf8)))
  #expect(
    try streams.finish(StreamReplayDoneEvent(streamId: "stream-a", nextOffset: "12"))
      == "pane-a")
  #expect(
    try streams.finish(StreamReplayDoneEvent(streamId: "stream-b", nextOffset: "42"))
      == "pane-b")
}

@Test func terminalStreamGapGenerationAndRemovalStayPaneLocal() throws {
  var streams = TerminalStreamReplica()
  try streams.install(
    paneId: "pane-a",
    info: StreamInfo(
      streamId: "stream-a", paneId: "pane-a", generation: 1,
      firstRetainedOffset: "0", nextOffset: "0", binaryPaneRef: 2))
  try streams.install(
    paneId: "pane-b",
    info: StreamInfo(
      streamId: "stream-b", paneId: "pane-b", generation: 2,
      firstRetainedOffset: "0", nextOffset: "0", binaryPaneRef: 3))
  _ = try streams.start(
    StreamReplayStartEvent(
      streamId: "stream-a", generation: 1, fromOffset: "0", toOffset: "5", reset: true))
  _ = try streams.start(
    StreamReplayStartEvent(
      streamId: "stream-b", generation: 2, fromOffset: "7", toOffset: "8", reset: true))

  #expect(
    try streams.gap(StreamGapEvent(streamId: "stream-a", fromOffset: "0", toOffset: "4"))
      == "pane-a")
  #expect(streams.gapText(for: "pane-a") != nil)
  #expect(streams.gapText(for: "pane-b") == nil)
  #expect(throws: ClientFailure.self) {
    try streams.accept(terminal(reference: 2, generation: 2, offset: 4, bytes: Data([1])))
  }
  #expect(
    try streams.accept(terminal(reference: 3, generation: 2, offset: 7, bytes: Data([2])))
      == TerminalStreamDelivery(paneId: "pane-b", bytes: Data([2])))

  streams.remove("pane-a")
  #expect(streams.count == 1)
  #expect(
    try streams.accept(terminal(reference: 2, generation: 1, offset: 4, bytes: Data([3]))) == nil)
  streams.retainOnly([])
  #expect(streams.count == 0)
}

@Test func emptyStreamGapDoesNotClaimOutputWasLost() throws {
  var streams = TerminalStreamReplica()
  try streams.install(
    paneId: "pane-a",
    info: StreamInfo(
      streamId: "stream-a", paneId: "pane-a", generation: 1,
      firstRetainedOffset: "0", nextOffset: "0", binaryPaneRef: 2))
  _ = try streams.start(
    StreamReplayStartEvent(
      streamId: "stream-a", generation: 1, fromOffset: "0", toOffset: "0", reset: true))

  #expect(
    try streams.gap(StreamGapEvent(streamId: "stream-a", fromOffset: "0", toOffset: "0")) == nil)
  #expect(streams.gapText(for: "pane-a") == nil)
  #expect(
    try streams.gap(StreamGapEvent(streamId: "stream-a", fromOffset: "0", toOffset: "1"))
      == "pane-a")
  #expect(streams.gapText(for: "pane-a") == TerminalStatePresentation.recordedGap.text)
}

@Test func terminalStreamReplacementRejectsReferenceAndStreamAliasing() throws {
  var streams = TerminalStreamReplica()
  try streams.install(
    paneId: "pane-a",
    info: StreamInfo(
      streamId: "stream-a", paneId: "pane-a", generation: 1,
      firstRetainedOffset: "0", nextOffset: "0", binaryPaneRef: 2))
  #expect(throws: ClientFailure.self) {
    try streams.install(
      paneId: "pane-b",
      info: StreamInfo(
        streamId: "stream-b", paneId: "pane-b", generation: 1,
        firstRetainedOffset: "0", nextOffset: "0", binaryPaneRef: 2))
  }
  #expect(throws: ClientFailure.self) {
    try streams.install(
      paneId: "pane-b",
      info: StreamInfo(
        streamId: "stream-a", paneId: "pane-b", generation: 1,
        firstRetainedOffset: "0", nextOffset: "0", binaryPaneRef: 3))
  }
  try streams.install(
    paneId: "pane-a",
    info: StreamInfo(
      streamId: "stream-a-2", paneId: "pane-a", generation: 2,
      firstRetainedOffset: "0", nextOffset: "0", binaryPaneRef: 4))
  #expect(streams.count == 1)
}

@Test func fixtureSocketCannotSelectAnInstalledProfile() {
  #expect(
    ActiveProfile.fixtureSocket("/tmp/sn-ui-123/engine.sock") == "/tmp/sn-ui-123/engine.sock")
  let installed = ActiveProfile.dataDirectory.appendingPathComponent("engine.sock").path
  #expect(ActiveProfile.fixtureSocket(installed) == nil)
  #expect(ActiveProfile.fixtureSocket("/tmp/engine.sock") == nil)
  #expect(ActiveProfile.fixtureSocket("/tmp/sn-ui-123/../other/engine.sock") == nil)
}

@Test func theProfileIsNamedByTheEnvironmentThenByTheBundleThenIsDev() {
  let env = ProfileValues.selectionEnvironment
  #expect(ProfileValues.selected(environment: [:], bundleIdentifier: nil).name == "dev")
  #expect(
    ProfileValues.selected(environment: [:], bundleIdentifier: ProfileValues.prod.appBundleId)
      .name == "prod")
  #expect(
    ProfileValues.selected(
      environment: [env: "dev"], bundleIdentifier: ProfileValues.prod.appBundleId
    ).name == "dev")
  #expect(
    ProfileValues.selected(environment: [env: "prod"], bundleIdentifier: nil).name == "prod")
  #expect(
    ProfileValues.selected(environment: [env: "other"], bundleIdentifier: "com.example.app").name
      == "dev")
}

@Test func everyProfileKeepsItsDataApartAndBelongsToTheNextFamily() {
  let profiles = ProfileValues.all
  #expect(Set(profiles.map(\.applicationSupportSubpath)).count == profiles.count)
  #expect(Set(profiles.map(\.tcpPort)).count == profiles.count)
  for profile in profiles {
    #expect(profile.applicationSupportSubpath.hasPrefix("Soyeht.next"))
    #expect(profile.appBundleId.contains(".next."))
  }
}
