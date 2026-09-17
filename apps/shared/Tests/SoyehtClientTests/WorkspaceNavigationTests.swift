import SoyehtProtocol
import Testing

@testable import SoyehtClient

private final class MemoryClientPreferenceStore: ClientPreferenceStore {
  private var values: [String: Bool] = [:]

  func bool(forKey key: String) -> Bool {
    values[key] ?? false
  }

  func set(_ value: Bool, forKey key: String) {
    values[key] = value
  }
}

@Test func writingDestinationsMapToOneClosedSectionAndKind() {
  #expect(WorkspaceDestination.texts.writingSection == .texts)
  #expect(WorkspaceDestination.presentations.writingSection == .presentations)
  #expect(WorkspaceDestination.screenplays.writingSection == .screenplays)
  #expect(WorkspaceDestination.editor.writingSection == nil)
  #expect(WritingSection.texts.destination == .texts)
  #expect(WritingSection.presentations.writingKind == .presentation)
  #expect(WritingSection.screenplays.writingKind == .screenplay)
}

@Test
@MainActor
func appBarVisibilityPersistsWithoutPersistingTheSelectedView() {
  let preferences = MemoryClientPreferenceStore()
  let navigation = WorkspaceNavigation(preferences: preferences)

  #expect(navigation.appBarOpen == false)
  #expect(navigation.destination == .agents)

  navigation.toggleAppBar()
  navigation.select(.web)

  #expect(navigation.appBarOpen)
  #expect(navigation.destination == .web)

  let restored = WorkspaceNavigation(preferences: preferences)
  #expect(restored.appBarOpen)
  #expect(restored.destination == .agents)
}

@Test func threePaneWritingCompositionPromotesTheSelectedSurfaceAndItsAgent() {
  let terminal = pane("terminal", mode: .terminal)
  let agent = pane("agent", mode: .agent)
  let text = pane("text", mode: .writing, kind: .text, section: .texts)
  let screenplay = pane(
    "screenplay", mode: .writing, kind: .screenplay, section: .screenplays)

  let selectedWriting = PanePresentationOrder.arrange(
    [terminal, agent, text, screenplay], selectedPaneId: screenplay.paneId)
  #expect(selectedWriting.map(\.paneId) == ["screenplay", "agent", "terminal", "text"])

  let selectedAgent = PanePresentationOrder.arrange(
    [terminal, text, agent], selectedPaneId: agent.paneId)
  #expect(selectedAgent.map(\.paneId) == ["text", "agent", "terminal"])

  let selectedTerminal = PanePresentationOrder.arrange(
    [terminal, agent, text], selectedPaneId: terminal.paneId)
  #expect(selectedTerminal.map(\.paneId) == ["terminal", "agent", "text"])
}

@Test @MainActor func paneUpdatePreservesVisiblePaneOrder() {
  let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
  let left = pane("left", mode: .terminal)
  let right = pane("right", mode: .terminal)
  client.upsertPane(left)
  client.upsertPane(right)

  var updatedLeft = left
  updatedLeft.state = .working
  updatedLeft.phrase = "Working"
  client.upsertPane(updatedLeft)

  #expect(client.panes.map(\.paneId) == ["left", "right"])
  #expect(client.panes.first?.state == .working)
  #expect(client.panes.first?.phrase == "Working")
}

private func pane(
  _ paneId: String, mode: PaneMode, kind: WritingKind? = nil,
  section: WritingSection? = nil
) -> Pane {
  Pane(
    paneId: paneId, workspaceId: "workspace-1", name: paneId, cwd: "/workspace", mode: mode,
    state: .idle, source: .protocol, confidence: 1, phrase: "Ready", generation: 1, cols: 80,
    rows: 24, record: mode == .agent, writingKind: kind, writingSection: section)
}
