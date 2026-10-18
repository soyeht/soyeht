import Foundation
import SoyehtProtocol

/// One entry of the pane header's ticker, newest first. `time` is drawn in the mono face.
public struct AgentTickerItem: Equatable, Sendable, Identifiable {
  public let id: String
  public let text: String
  public let time: String?

  public init(id: String, text: String, time: String? = nil) {
    self.id = id
    self.text = text
    self.time = time
  }
}

/// What the ticker knows about a pane at one moment.
public struct AgentTickerContext: Sendable {
  public let pane: Pane?
  public let view: AgentPaneView
  /// When the running turn started, as this device saw it.
  public let turnStartedAt: Date?
  public let now: Date

  public init(pane: Pane?, view: AgentPaneView, turnStartedAt: Date?, now: Date) {
    self.pane = pane
    self.view = view
    self.turnStartedAt = turnStartedAt
    self.now = now
  }
}

/// The ticker's content: entries newest first, and the running turn's elapsed time, which
/// the header shows apart so the rolling line does not restart every second.
public struct AgentTickerSnapshot: Equatable, Sendable {
  public let items: [AgentTickerItem]
  public let elapsed: String?

  public init(items: [AgentTickerItem], elapsed: String?) {
    self.items = items
    self.elapsed = elapsed
  }
}

/// Feeds the pane header's one-line "what is happening" ticker. Today the ACP source reads
/// the session; a summarizer can conform later and the header needs no change.
public protocol AgentTickerSource: Sendable {
  func snapshot(for context: AgentTickerContext) -> AgentTickerSnapshot
}

/// The ticker from ACP data the engine already has: the session title, the plan step in
/// progress, the latest tool calls with their status and the running turn's elapsed time.
public struct AgentACPTickerSource: AgentTickerSource {
  /// Earlier tool calls listed after the current activity.
  public static let toolHistory = 3

  public init() {}

  public func snapshot(for context: AgentTickerContext) -> AgentTickerSnapshot {
    let messages = Self.latestTurn(context.view.messages)
    var items: [AgentTickerItem] = []
    let tools = Self.liveTools(messages)
    if let now = current(messages: messages, latestTool: tools.first) {
      items.append(now)
    }
    let history = tools.dropFirst(items.isEmpty ? 0 : 1).prefix(Self.toolHistory)
    items += history.map { tool in
      AgentTickerItem(id: "tool-\(tool.toolCallId)", text: Self.describe(tool))
    }
    if let title = context.pane?.title?.trimmingCharacters(in: .whitespacesAndNewlines),
      !title.isEmpty
    {
      items.append(AgentTickerItem(id: "title", text: title))
    }
    return AgentTickerSnapshot(items: items, elapsed: elapsed(context))
  }

  /// The messages since the person's last prompt. An earlier turn's tool calls and plan say
  /// nothing about what the agent is doing now, so a new turn starts the ticker clean.
  static func latestTurn(_ messages: [TranscriptMessage]) -> ArraySlice<TranscriptMessage> {
    let start = messages.lastIndex { $0.role == .user && $0.kind == .message }
    return messages[(start.map { $0 + 1 } ?? messages.startIndex)...]
  }

  /// The turn's tool calls, newest first, without a failure that a later call of the same
  /// turn has since got past: the retry succeeded, so the header no longer reports it.
  static func liveTools(_ messages: ArraySlice<TranscriptMessage>) -> [ToolCall] {
    var recovered = false
    var tools: [ToolCall] = []
    for tool in messages.reversed().compactMap(\.tool) {
      if tool.status == .failed, recovered { continue }
      if tool.status == .completed { recovered = true }
      tools.append(tool)
    }
    return tools
  }

  private func elapsed(_ context: AgentTickerContext) -> String? {
    guard let pane = context.pane, pane.state.isTurnRunning,
      let start = context.turnStartedAt
    else { return nil }
    return AgentElapsedText.text(seconds: max(0, Int(context.now.timeIntervalSince(start))))
  }

  /// The current activity: the plan step in progress and the latest tool call.
  private func current(messages: ArraySlice<TranscriptMessage>, latestTool: ToolCall?)
    -> AgentTickerItem?
  {
    var parts: [String] = []
    if let plan = messages.last(where: { $0.kind == .plan })?.plan, !plan.isEmpty,
      let step = plan.firstIndex(where: { $0.status == .in_progress })
        ?? plan.firstIndex(where: { $0.status == .pending })
    {
      parts.append("Step \(step + 1)/\(plan.count): \(plan[step].content)")
    }
    if let tool = latestTool {
      parts.append(Self.describe(tool))
    }
    guard !parts.isEmpty else { return nil }
    return AgentTickerItem(id: "now", text: parts.joined(separator: " · "))
  }

  static func describe(_ tool: ToolCall) -> String {
    switch tool.status {
    case .pending: "Waiting: \(tool.title)"
    case .in_progress: "Running \(tool.title)"
    case .completed: "Done: \(tool.title)"
    case .failed: "Failed: \(tool.title)"
    }
  }
}

/// When the running turn started, as this device saw it: the engine does not report a turn's
/// start, so the clock notes the moment the active prompt first appeared.
public struct AgentTurnClock: Equatable, Sendable {
  private var promptId: String?
  public private(set) var startedAt: Date?

  public init() {}

  public mutating func observe(activePromptId: String?, now: Date) {
    guard activePromptId != promptId else { return }
    promptId = activePromptId
    startedAt = activePromptId == nil ? nil : now
  }
}

/// The status word at the header's trailing edge, and whether it asks for the person.
public enum AgentHeaderStatus {
  public static func needsYou(state: PaneState?, view: AgentPaneView) -> Bool {
    state == .waiting_for_you || view.permission != nil || view.elicitation != nil
  }
}
