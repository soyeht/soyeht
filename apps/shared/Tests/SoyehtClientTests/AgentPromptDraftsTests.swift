import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@MainActor @Test func aDraftIsKeptPerPaneAndAnEmptyOneIsForgotten() {
  let drafts = AgentPromptDrafts()
  drafts.set("write the tests", for: "a")
  drafts.set("and the docs", for: "b")
  #expect(drafts.text(for: "a") == "write the tests")
  #expect(drafts.text(for: "b") == "and the docs")
  drafts.set("", for: "a")
  #expect(drafts.text(for: "a").isEmpty)
  #expect(drafts.text(for: "b") == "and the docs")
  #expect(drafts.text(for: "never typed").isEmpty)
}

@MainActor @Test func aDraftLeavesWithItsPane() throws {
  let client = EngineClient(socket: "/tmp/prompt-drafts.sock")
  let pane = PaneLayoutFixture.pane("draft-pane", workspace: "work", mode: .agent)
  try client.applyPaneCreated(
    PaneCreatedEvent(eventId: "1", at: "2026-11-02T12:00:00Z", pane: pane))
  AgentPromptDrafts.shared.set("half a thought", for: "draft-pane")

  try client.applyPaneEvent(.paneClosed, .object(["paneId": .string("draft-pane")]))

  #expect(AgentPromptDrafts.shared.text(for: "draft-pane").isEmpty)
}

@MainActor @Test func attachedFilesSurviveARebuiltComposerAndLeaveWithTheirPane() async throws {
  let drafts = AgentPromptDrafts()
  let first = drafts.attachments(for: "a", linkHostFiles: false)
  first.attach(data: Data("notes".utf8), type: .plainText, name: "notes.txt")
  var polls = 0
  while first.items.isEmpty && polls < 1_000 {
    polls += 1
    try await Task.sleep(for: .milliseconds(10))
  }
  #expect(first.items.count == 1)

  // A rotation or a keyboard builds the pane's view again and asks for its attachments again.
  let again = drafts.attachments(for: "a", linkHostFiles: false)
  #expect(again === first)
  #expect(again.items.map(\.name) == ["notes.txt"])
  #expect(drafts.attachments(for: "b", linkHostFiles: false).items.isEmpty)

  // Sending takes them; the next composer starts empty.
  #expect(again.take().count == 1)
  #expect(drafts.attachments(for: "a", linkHostFiles: false).items.isEmpty)

  first.attach(data: Data("more".utf8), type: .plainText, name: "more.txt")
  while first.items.isEmpty && polls < 2_000 {
    polls += 1
    try await Task.sleep(for: .milliseconds(10))
  }
  drafts.discard("a")
  #expect(drafts.attachments(for: "a", linkHostFiles: false) !== first)
}
