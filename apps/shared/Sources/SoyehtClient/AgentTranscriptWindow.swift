import Foundation
import SoyehtProtocol

/// How much of a long conversation a replica holds (ADR 0167). The engine keeps the whole
/// transcript and pages it, so a client that lets go of the oldest messages can fetch them again
/// with "Load earlier"; what a pane costs follows what is on screen, not how long it has run.
///
/// The rule is `window` and `cursor` of `contracts/vectors/agent-transcript.json`, which the
/// desktop client passes as well.
enum AgentTranscriptWindow {
  /// The sizes of the window: `ClientPolicy.capsHeldMessages` for a pane that is shown and
  /// `ClientPolicy.capsOffscreenMessages` for one that is not. A merge trims only past `slack`
  /// more, back down to the size, so a streaming pane does not rebuild its transcript for each
  /// message; leaving the screen trims at once.
  struct Limits {
    var held = ClientPolicy.capsHeldMessages
    var offscreen = ClientPolicy.capsOffscreenMessages
    var slack = Int(ClientPolicy.pageSizeTranscript)
  }

  /// The index before which a message is let go of, unless it is still needed: the newest
  /// `limit` messages are kept, and a person who scrolled back also keeps the half of a window
  /// before the message where they were, in view or left behind. A message that is not held
  /// counts as the newest.
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

  /// Whether the pane still needs a message that is old enough to go: in the latest turn (after
  /// the person's last message), a reply or subagent that has not finished and a tool call that has
  /// not, and anywhere the newest usage report (the context ring reads it). What an earlier turn
  /// left unfinished is stale, and a command's output is never final in a live update.
  static func isNeeded(
    _ message: TranscriptMessage, at index: Int, newestUsage: Int?, turnStart: Int
  ) -> Bool {
    if message.kind == .usage { return index == newestUsage }
    guard index >= turnStart else { return false }
    if let status = message.tool?.status { return status != .completed && status != .failed }
    return !message.complete && message.kind != .terminal
  }

  /// The cursor of "Load earlier" after a page: the older of the one held and the page's, so a
  /// reload of the newest page does not move it past messages the replica does not hold.
  static func older(_ held: String?, _ page: String?) -> String? {
    guard let page else { return nil }
    guard let held, let heldSeq = UInt64(held), let pageSeq = UInt64(page) else { return page }
    return heldSeq <= pageSeq ? held : page
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
  /// Lets go of the oldest messages beyond the window (see `AgentTranscriptWindow`) of the pane as
  /// it is shown or not, with the place the person was recorded.
  mutating func trimToWindow(force: Bool = false) {
    var viewing: String?
    if case .row(let id) = scroll.place { viewing = id }
    trim(shown: scroll.isShown, viewing: viewing, force: force)
  }

  /// Points "Load earlier" at the oldest message kept after the last one let go of, so everything
  /// dropped can be fetched again. `viewing` is the row the person was reading, none at the newest.
  mutating func trim(
    shown: Bool, viewing: String?, limits: AgentTranscriptWindow.Limits = .init(),
    force: Bool = true
  ) {
    let limit = shown ? limits.held : limits.offscreen
    guard messages.count > (force ? limit : limit + limits.slack) else { return }
    let start = AgentTranscriptWindow.start(messages, limit: limit, viewing: viewing)
    let newestUsage = messages.lastIndex { $0.kind == .usage }
    let turnStart = messages.lastIndex { $0.role == .user && $0.kind == .message } ?? 0
    var kept: [TranscriptMessage] = []
    var lastLetGo: Int?
    for (index, message) in messages.enumerated() {
      if index < start,
        !AgentTranscriptWindow.isNeeded(
          message, at: index, newestUsage: newestUsage, turnStart: turnStart)
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
