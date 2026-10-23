import Foundation
import SoyehtProtocol
import os

public struct AgentPaneView: Sendable {
  public var messages: [TranscriptMessage] = []
  /// The revision each message was last written at. A transcript row is redrawn only when its
  /// message's revision moved, so an update costs the changed message, not the transcript. The
  /// counter lives outside the view, so a revision is never reused: not after a replaced page,
  /// and not when a disconnect drops the view and a new one is made for the same pane.
  public internal(set) var revisions: [String: UInt64] = [:]
  private static let revisionClock = OSAllocatedUnfairLock(initialState: UInt64(0))
  public var permission: PermissionRequest?
  public var elicitation: ElicitationRequest?
  public var queue: [PromptItem] = []
  public var queuePaused = false
  public internal(set) var nextTranscriptCursor: String?
  public var hasOlderMessages: Bool { nextTranscriptCursor != nil }
  public var notice: String?
  public var authMethods: [AuthMethod] = []
  /// Why the last sign-in did not finish; cleared when the next one starts or succeeds.
  public var signInFailure: String?
  public var harnessSwitch: AgentHarnessSwitchView?
  /// Where the transcript was left; a reference, so scrolling does not redraw every pane.
  public let scroll = AgentTranscriptScroll()

  public init() {}

  /// How `merge` placed a message. A streamed reply is the newest message, so it is `.replacedLast`
  /// or `.appended` and costs the same at any transcript length; only an older message needs a search.
  public enum MergeCost: Equatable, Sendable {
    case replacedLast, appended, searched
  }

  public func revision(of localId: String) -> UInt64 { revisions[localId] ?? 0 }

  private static func order(_ message: TranscriptMessage) -> (UInt64, String) {
    (UInt64(message.seq) ?? 0, message.localId)
  }

  /// `revisions` knows every message only while the transcript was built through `merge`; a
  /// transcript assigned whole falls back to a search that learns the missing ones.
  private var tracksEveryMessage: Bool { revisions.count == messages.count }

  private mutating func stamp(_ localId: String) {
    revisions[localId] = Self.revisionClock.withLock { clock in
      clock &+= 1
      return clock
    }
  }

  @discardableResult mutating func merge(_ message: TranscriptMessage) -> MergeCost {
    if tracksEveryMessage, let last = messages.last {
      if last.localId == message.localId, Self.order(last) == Self.order(message) {
        messages[messages.count - 1] = message
        stamp(message.localId)
        return .replacedLast
      }
      if revisions[message.localId] == nil, Self.order(last) < Self.order(message) {
        messages.append(message)
        stamp(message.localId)
        return .appended
      }
    }
    replaceOrAdd(message)
    messages.sort { Self.order($0) < Self.order($1) }
    return .searched
  }

  private mutating func replaceOrAdd(_ message: TranscriptMessage) {
    if let index = messages.firstIndex(where: { $0.localId == message.localId }) {
      messages[index] = message
    } else {
      messages.append(message)
    }
    for known in messages where revisions[known.localId] == nil { stamp(known.localId) }
    stamp(message.localId)
  }

  /// A page of history: sorted once, not once per message. A page can be older than what the view
  /// holds (it was read before the last updates), so it never undoes a message that is further
  /// along; a live update, which carries the whole message, always replaces.
  mutating func merge(_ page: [TranscriptMessage]) {
    guard !page.isEmpty else { return }
    var index = Dictionary(
      messages.enumerated().map { ($1.localId, $0) }, uniquingKeysWith: { _, latest in latest })
    var changed: [String] = []
    for message in page {
      if let known = index[message.localId] {
        guard message.supersedes(messages[known]) else { continue }
        messages[known] = message
      } else {
        index[message.localId] = messages.count
        messages.append(message)
      }
      changed.append(message.localId)
    }
    messages.sort { Self.order($0) < Self.order($1) }
    for known in messages where revisions[known.localId] == nil { stamp(known.localId) }
    for localId in changed { stamp(localId) }
  }

  mutating func mergeTranscriptPage(_ page: PaneTranscriptResult, replacing: Bool) {
    if replacing {
      messages = []
      revisions = [:]
    }
    merge(page.items)
    nextTranscriptCursor = page.nextCursor
  }
}

extension TranscriptMessage {
  /// Whether this message is at least as far along as `known`, the one held with the same id: a
  /// finished message beats an unfinished one, a tool call further on beats one behind, and a
  /// longer text (in bytes) beats a shorter. A command's output has no text, so the one with more
  /// bytes beats the one with fewer.
  func supersedes(_ known: TranscriptMessage) -> Bool {
    if complete != known.complete { return complete }
    if complete { return true }
    if let tool, let heldTool = known.tool {
      return tool.status.progress >= heldTool.status.progress
    }
    if let terminal, let heldTerminal = known.terminal {
      return terminal.bytes.utf8.count >= heldTerminal.bytes.utf8.count
    }
    return text.utf8.count >= known.text.utf8.count
  }
}

extension ToolStatus {
  /// How far a tool call has got: waiting, running, or finished (completed or failed).
  fileprivate var progress: Int {
    switch self {
    case .pending: 0
    case .in_progress: 1
    case .completed, .failed: 2
    }
  }
}
