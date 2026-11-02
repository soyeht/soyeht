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
