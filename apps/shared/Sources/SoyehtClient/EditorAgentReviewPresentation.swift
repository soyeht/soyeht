import Foundation
import SoyehtProtocol

public struct EditorAgentReviewHunk: Identifiable, Sendable {
  public let id: String
  public let line: UInt32
  public let changedLineCount: Int
  public let summary: String
  public let authorName: String

  public init(
    id: String, line: UInt32, changedLineCount: Int, summary: String, authorName: String
  ) {
    self.id = id
    self.line = line
    self.changedLineCount = changedLineCount
    self.summary = summary
    self.authorName = authorName
  }
}

public struct EditorAgentReviewPresentation: Sendable {
  public let revision: String
  public let paneId: String
  public let hunks: [EditorAgentReviewHunk]
  public let lines: Set<UInt32>

  public init(_ event: EditorReviewRequestedEvent, fallbackName: String = "Agent") {
    revision = event.revision
    paneId = event.paneId
    hunks = event.hunks.map { hunk in
      let changed = hunk.lines.filter { $0.kind != .context }
      let summary =
        changed.first.map {
          String($0.text.trimmingCharacters(in: .whitespacesAndNewlines).prefix(80))
        } ?? "Change"
      let authorName = changed.compactMap { $0.author?.name }.first ?? fallbackName
      return EditorAgentReviewHunk(
        id: hunk.hunkId,
        line: changed.compactMap(\.newLine).min() ?? hunk.newStart,
        changedLineCount: changed.count,
        summary: summary,
        authorName: authorName)
    }
    lines = Set(hunks.map(\.line))
  }
}
