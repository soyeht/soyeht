import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func peerMessageSenderIsShownByNameAndNeverByPaneIdentifier() {
  let named = AgentMessageSource(paneId: "pane-7", name: "Claude Agent", harness: "Claude Agent")
  #expect(named.displayName == "Claude Agent")

  let unnamed = AgentMessageSource(paneId: "pane-7", name: "  ", harness: "Codex")
  #expect(unnamed.displayName == "Codex")
  #expect(!unnamed.displayName.contains("pane-7"))
}
