import Foundation
import SoyehtProtocol
import SwiftTerm
import Testing

@testable import SoyehtClient

private func streamInfo() -> StreamInfo {
  StreamInfo(
    streamId: "stream-a", paneId: "pane-a", generation: 1,
    firstRetainedOffset: "0", nextOffset: "0", binaryPaneRef: 2)
}

@Test func aStreamIsReplayingBetweenReplayStartAndReplayDone() throws {
  var streams = TerminalStreamReplica()
  try streams.install(paneId: "pane-a", info: streamInfo())
  #expect(!streams.isReplaying("pane-a"))

  _ = try streams.start(
    StreamReplayStartEvent(
      streamId: "stream-a", generation: 1, fromOffset: "0", toOffset: "0", reset: true))
  #expect(streams.isReplaying("pane-a"))
  #expect(!streams.isReplaying("pane-b"))

  _ = try streams.finish(StreamReplayDoneEvent(streamId: "stream-a", nextOffset: "0"))
  #expect(!streams.isReplaying("pane-a"))
}

@Test func aStreamThatIsRemovedMidReplayIsNoLongerReplaying() throws {
  var streams = TerminalStreamReplica()
  try streams.install(paneId: "pane-a", info: streamInfo())
  _ = try streams.start(
    StreamReplayStartEvent(
      streamId: "stream-a", generation: 1, fromOffset: "0", toOffset: "0", reset: true))
  streams.remove("pane-a")
  #expect(!streams.isReplaying("pane-a"))
}

@MainActor @Test func aReplayedCopyRequestNeverReachesTheClipboard() {
  let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
  var copied: [String] = []
  var replaying = true
  let host = TerminalHost(
    client: client, acceptsInput: true,
    writeClipboard: { text, _ in copied.append(text) }, isReplaying: { replaying })
  let view = TerminalView(frame: .zero)
  host.attach(view, refresh: false)
  // The same bytes, as an OSC 52 write the program sent: base64("copied text") then BEL.
  let sequence = Array("\u{1b}]52;c;Y29waWVkIHRleHQ=\u{07}".utf8)

  host.clipboardCopy(source: view, content: Data("direct".utf8))
  view.feed(byteArray: sequence[...])
  #expect(copied.isEmpty)

  replaying = false
  host.clipboardCopy(source: view, content: Data("direct".utf8))
  view.feed(byteArray: sequence[...])
  #expect(copied == ["direct", "copied text"])
}
