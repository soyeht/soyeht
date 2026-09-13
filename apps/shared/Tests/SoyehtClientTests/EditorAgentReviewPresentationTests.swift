import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func agentReviewProjectsAuthorshipSummaryAndBoundedGutterAnchors() {
  let author = FileAuthor(
    actor: Actor(role: .agent, client: "fixture", paneId: "agent-1"), name: "Worker")
  let event = EditorReviewRequestedEvent(
    eventId: "1", at: "2026-09-13T04:00:00Z", fileId: "file-1", revision: "7",
    paneId: "agent-1",
    hunks: [
      DiffHunk(
        hunkId: "replace", oldStart: 3, oldLines: 1, newStart: 3, newLines: 1,
        lines: [
          DiffLine(kind: .removed, oldLine: 3, text: "old", author: author),
          DiffLine(kind: .added, newLine: 3, text: "new value", author: author),
        ]),
      DiffHunk(
        hunkId: "delete", oldStart: 8, oldLines: 1, newStart: 8, newLines: 0,
        lines: [DiffLine(kind: .removed, oldLine: 8, text: "gone")]),
    ])

  let presentation = EditorAgentReviewPresentation(event, fallbackName: "Fallback")

  #expect(presentation.revision == "7")
  #expect(presentation.paneId == "agent-1")
  #expect(presentation.lines == Set([3, 8]))
  #expect(presentation.hunks[0].authorName == "Worker")
  #expect(presentation.hunks[0].changedLineCount == 2)
  #expect(presentation.hunks[0].summary == "old")
  #expect(presentation.hunks[1].authorName == "Fallback")
}
