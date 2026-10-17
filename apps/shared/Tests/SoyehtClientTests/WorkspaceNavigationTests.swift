import SoyehtProtocol
import Testing

@testable import SoyehtClient

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
  let preferences = MemoryPreferenceStore()
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

@Test @MainActor func workspaceUpdatePreservesCreationOrder() {
  let client = EngineClient(socket: "/tmp/sn-render-only/engine.sock")
  client.upsertWorkspace(workspace("last-created", name: "Last created"))
  client.upsertWorkspace(workspace("first-alphabetically", name: "First alphabetically"))
  client.upsertWorkspace(workspace("middle", name: "Middle"))

  client.upsertWorkspace(workspace("first-alphabetically", name: "Renamed"))

  #expect(
    client.workspaces.map(\.workspaceId)
      == ["last-created", "first-alphabetically", "middle"])
  #expect(client.workspaces[1].name == "Renamed")
}

private func workspace(_ workspaceId: String, name: String) -> Workspace {
  Workspace(workspaceId: workspaceId, name: name, path: "/\(workspaceId)", layout: .object([:]))
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

@Test func onlyPaneGridsDrawUnderAnOverlaidRail() {
  let grids = WorkspaceDestination.allCases.filter(\.drawsUnderOverlaidRail)
  #expect(Set(grids) == [.agents, .editor, .web, .terminal])
  #expect(!WorkspaceDestination.presentations.drawsUnderOverlaidRail)
  #expect(!WorkspaceDestination.tasks.drawsUnderOverlaidRail)
}
