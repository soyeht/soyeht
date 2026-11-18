import Foundation
import SoyehtProtocol

/// How much of a long conversation a replica holds (ADR 0167). The engine keeps the whole
/// transcript and pages it, so a client that lets go of the oldest messages can fetch them again
/// with "Load earlier"; what a pane costs follows what is on screen, not how long it has run.
///
/// The rule is `window` of `contracts/vectors/agent-transcript.json`, which the desktop client
/// passes as well.
enum AgentTranscriptWindow {
  /// The sizes of the window: `ClientPolicy.capsHeldMessages` for a pane that is shown and
  /// `ClientPolicy.capsOffscreenMessages` for one that is not.
  struct Limits {
    var held = ClientPolicy.capsHeldMessages
    var offscreen = ClientPolicy.capsOffscreenMessages
  }

  /// The index before which a message is let go of, unless it is still needed: the newest
  /// `limit` messages are kept, and a person who scrolled back also keeps the half of a window
  /// before the message in view. A message in view that is not held counts as the newest.
  static func start(_ messages: [TranscriptMessage], limit: Int, viewing: String?) -> Int {
    let newest = max(0, messages.count - limit)
    guard let viewing, let index = messages.firstIndex(where: { place($0) == viewing })
    else { return newest }
    return min(newest, max(0, index - limit / 2))
  }

  /// The id of the transcript row a message is drawn in: a subagent's run is the row of its group.
  private static func place(_ message: TranscriptMessage) -> String {
    message.subagent.map { "subagent:\($0.subagentId)" } ?? message.localId
  }

  /// Whether the pane still needs a message that is old enough to go: a reply or a subagent that
  /// has not finished, a tool call that has not, and the newest usage report (the context ring
  /// reads it). A command's output is never final in a live update, so it does not hold one.
  static func isNeeded(_ message: TranscriptMessage, isNewestUsage: Bool) -> Bool {
    if message.kind == .usage { return isNewestUsage }
    if let status = message.tool?.status, status != .completed, status != .failed { return true }
    return !message.complete && message.kind != .terminal
  }
}

extension TranscriptMessage {
  /// What the client keeps of a message: a tool call's content and raw input and output are
  /// trees no view reads, and they are the largest part of a long run's tool calls.
  var light: TranscriptMessage {
    guard var tool else { return self }
    tool.content = nil
    tool.rawInput = nil
    tool.rawOutput = nil
    var message = self
    message.tool = tool
    return message
  }
}

extension AgentPaneView {
  /// Lets go of the oldest messages beyond the window of a pane shown or not (see
  /// `AgentTranscriptWindow`), and points "Load earlier" at the oldest message kept after the
  /// last one let go of, so everything dropped can be fetched again.
  mutating func trimToWindow() {
    var viewing: String?
    if case .row(let id) = scroll.place { viewing = id }
    trim(shown: scroll.isShown, viewing: viewing)
  }

  /// `viewing` is the message the person is looking at, none at the newest; a pane that is not
  /// shown has nobody looking.
  mutating func trim(
    shown: Bool, viewing: String?, limits: AgentTranscriptWindow.Limits = .init()
  ) {
    let limit = shown ? limits.held : limits.offscreen
    guard messages.count > limit else { return }
    let start = AgentTranscriptWindow.start(
      messages, limit: limit, viewing: shown ? viewing : nil)
    let newestUsage = messages.lastIndex { $0.kind == .usage }
    var kept: [TranscriptMessage] = []
    var lastLetGo: Int?
    for (index, message) in messages.enumerated() {
      if index < start,
        !AgentTranscriptWindow.isNeeded(message, isNewestUsage: index == newestUsage)
      {
        lastLetGo = index
        revisions[message.localId] = nil
      } else {
        kept.append(message)
      }
    }
    guard let lastLetGo else { return }
    nextTranscriptCursor = messages[lastLetGo + 1].seq
    messages = kept
  }
}
