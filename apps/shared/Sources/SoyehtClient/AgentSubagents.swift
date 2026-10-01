import Foundation
import SoyehtProtocol
import SwiftUI

/// One entry of a displayed agent transcript: a message, or a subagent row
/// holding the activity that subagent owns.
public enum AgentTranscriptEntry: Identifiable, Sendable {
  case message(TranscriptMessage)
  case subagent(AgentSubagentGroup)

  public var id: String {
    switch self {
    case .message(let message): message.localId
    case .subagent(let group): "subagent:\(group.subagentId)"
    }
  }
}

public struct AgentSubagentGroup: Sendable {
  public let subagentId: String
  /// The subagent's row; absent while only a later transcript page is loaded.
  public let run: SubagentRun?
  public let entries: [AgentTranscriptEntry]
}

/// Nests each subagent's messages under its row, so nothing a subagent does
/// reads as the parent agent's own.
public enum AgentTranscriptGrouping {
  public static func entries(_ messages: [TranscriptMessage]) -> [AgentTranscriptEntry] {
    let owned = Dictionary(grouping: messages.filter { $0.subagentId != nil }) {
      $0.subagentId ?? ""
    }
    let rows = Set(messages.compactMap { $0.subagent?.subagentId })
    var placed = Set<String>()
    var result: [AgentTranscriptEntry] = []
    for message in messages {
      if let owner = message.subagentId {
        // A page can hold a subagent's activity without the row it follows.
        if !rows.contains(owner), placed.insert(owner).inserted {
          result.append(.subagent(group(owner, run: nil, owned: owned)))
        }
      } else if let run = message.subagent {
        result.append(.subagent(group(run.subagentId, run: run, owned: owned)))
      } else {
        result.append(.message(message))
      }
    }
    return result
  }

  /// How many subagents the parent is waiting on: the running rows that end
  /// the transcript, before the parent says anything else. Harness "wait"
  /// calls on subagents fold into this instead of showing as tool rows.
  public static func awaited(_ entries: [AgentTranscriptEntry]) -> Int {
    var running = 0
    for entry in entries.reversed() {
      guard case .subagent(let group) = entry else { break }
      if group.run?.status == .running { running += 1 }
    }
    return running
  }

  private static func group(
    _ id: String, run: SubagentRun?, owned: [String: [TranscriptMessage]]
  ) -> AgentSubagentGroup {
    let entries = (owned[id] ?? []).map { message -> AgentTranscriptEntry in
      if let nested = message.subagent {
        return .subagent(group(nested.subagentId, run: nested, owned: owned))
      }
      return .message(message)
    }
    return AgentSubagentGroup(subagentId: id, run: run, entries: entries)
  }
}

/// Which subagent rows are open. Rows start closed so parallel subagents stay
/// one line each; opening one shows its activity.
public struct AgentSubagentDisclosure: Equatable, Sendable {
  private var expanded: Set<String> = []

  public init() {}

  public func isExpanded(_ subagentId: String) -> Bool { expanded.contains(subagentId) }

  public mutating func toggle(_ subagentId: String) {
    if expanded.remove(subagentId) == nil { expanded.insert(subagentId) }
  }
}

public struct AgentSubagentPresentation: Sendable {
  public enum Status: Equatable, Sendable {
    case running, succeeded, failed, stopped
  }

  public let name: String
  public let model: String?
  public let status: Status
  public let trailing: String
  public let summary: String?

  public init(_ group: AgentSubagentGroup, now: Date = Date()) {
    let run = group.run
    name = run?.name ?? "Subagent"
    model = run?.model
    summary = run?.summary.flatMap { AgentSubagentPreview.text($0) }
    switch run?.status {
    case .running:
      status = .running
      trailing =
        run?.startedAt.flatMap(UsageTimestamp.date(from:)).map {
          AgentElapsedText.text(seconds: Int(max(now.timeIntervalSince($0), 0)))
        } ?? "running"
    case .completed:
      status = .succeeded
      trailing = "done"
    case .failed:
      status = .failed
      trailing = "failed"
    case .disconnected:
      status = .failed
      trailing = "disconnected"
    case .cancelled:
      status = .stopped
      trailing = "cancelled"
    case nil:
      status = .stopped
      trailing = ""
    }
  }

  /// The subtle status shown while the parent waits on its subagents.
  public static func waiting(_ count: Int) -> String {
    "Waiting for \(count) \(count == 1 ? "subagent" : "subagents")"
  }
}

/// The collapsed row's preview of a subagent's last message: its meaningful
/// content on one line. Lead-in lines ending in ":" ("Contents verbatim:"),
/// code fences, list markers, and runs of whitespace are dropped; lines are
/// joined with " · " and the result is cut at a word with an ellipsis.
public enum AgentSubagentPreview {
  public static let limit = 160

  public static func text(_ summary: String, limit: Int = limit) -> String? {
    let lines = summary.split(whereSeparator: \.isNewline).compactMap(line)
    let content = lines.filter { !$0.hasSuffix(":") }
    let joined = (content.isEmpty ? lines : content).joined(separator: " · ")
    return joined.isEmpty ? nil : truncated(joined, limit: limit)
  }

  private static func line(_ raw: Substring) -> String? {
    var text = raw.trimmingCharacters(in: .whitespaces)
    if text.hasPrefix("```") || text.hasPrefix("~~~") { return nil }
    for marker in ["- ", "* ", "+ ", "• "] where text.hasPrefix(marker) {
      text.removeFirst(marker.count)
    }
    if text.hasPrefix("#") { text = String(text.drop { $0 == "#" }) }
    let words = text.split(whereSeparator: \.isWhitespace)
    return words.isEmpty ? nil : words.joined(separator: " ")
  }

  private static func truncated(_ text: String, limit: Int) -> String {
    guard text.count > limit else { return text }
    var cut = String(text.prefix(limit - 1))
    // Prefer ending at a word unless that drops most of the line.
    if let space = cut.lastIndex(of: " "), cut.distance(from: cut.startIndex, to: space) > limit / 2
    {
      cut = String(cut[..<space])
    }
    let trimmed = cut.trimmingCharacters(in: .whitespaces.union(.punctuationCharacters))
    return (trimmed.isEmpty ? cut : trimmed) + "…"
  }
}

/// The parent's wait on its subagents, folded into one subtle line under them.
struct AgentSubagentWaitingStatus: View {
  let entries: [AgentTranscriptEntry]
  let style: AgentConversationStyle

  var body: some View {
    let awaited = AgentTranscriptGrouping.awaited(entries)
    if awaited > 0 {
      Text(AgentSubagentPresentation.waiting(awaited))
        .font(style.captionFont).foregroundStyle(style.secondary)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityIdentifier("agent-subagent-waiting")
    }
  }
}

struct AgentSubagentRow<Entry: View>: View {
  let group: AgentSubagentGroup
  let style: AgentConversationStyle
  @Binding var disclosure: AgentSubagentDisclosure
  let entry: (AgentTranscriptEntry) -> Entry

  private var expanded: Bool { disclosure.isExpanded(group.subagentId) }

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      Button {
        disclosure.toggle(group.subagentId)
      } label: {
        if group.run?.status == .running {
          // Only a running row has a clock to advance.
          TimelineView(.periodic(from: .now, by: 1)) { timeline in
            header(AgentSubagentPresentation(group, now: timeline.date))
          }
        } else {
          header(AgentSubagentPresentation(group))
        }
      }
      .buttonStyle(.plain)
      .accessibilityIdentifier("agent-subagent-row")
      .accessibilityHint(
        expanded ? "Hides the subagent's activity" : "Shows the subagent's activity")
      if expanded {
        VStack(alignment: .leading, spacing: 8) {
          if let task = group.run?.task, !task.isEmpty {
            Text(task).font(style.captionFont).foregroundStyle(style.secondary)
              .textSelection(.enabled)
          }
          ForEach(group.entries) { child in entry(child) }
        }
        .padding(.leading, 22)
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }

  private func header(_ presentation: AgentSubagentPresentation) -> some View {
    VStack(alignment: .leading, spacing: 3) {
      HStack(spacing: 8) {
        Image(systemName: expanded ? "chevron.down" : "chevron.right")
          .font(style.captionFont).foregroundStyle(style.secondary).frame(width: 10)
        statusIcon(presentation.status).frame(width: 14, height: 14)
        Text(presentation.name).fontWeight(.semibold).foregroundStyle(style.ink)
          .lineLimit(1)
        if let model = presentation.model {
          Text("· \(model)").foregroundStyle(style.secondary).lineLimit(1)
        }
        Spacer(minLength: 8)
        Text(presentation.trailing).font(style.captionFont).foregroundStyle(style.secondary)
          .monospacedDigit()
      }
      if !expanded, let summary = presentation.summary {
        Text(summary).font(style.captionFont).foregroundStyle(style.secondary).lineLimit(2)
          .padding(.leading, 40)
      }
    }
    .contentShape(Rectangle())
  }

  @ViewBuilder private func statusIcon(_ status: AgentSubagentPresentation.Status) -> some View {
    switch status {
    case .running:
      ProgressView().controlSize(.small).tint(style.accent)
    case .succeeded:
      Image(systemName: "checkmark").foregroundStyle(style.success)
    case .failed:
      Image(systemName: "xmark").foregroundStyle(style.danger)
    case .stopped:
      Image(systemName: "xmark").foregroundStyle(style.secondary)
    }
  }
}
