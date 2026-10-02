import SoyehtProtocol
import Testing

@testable import SoyehtClient

/// Footprint floors as the Mac derives them: the editor floor includes the
/// project panel beside the code.
private let slotSizing = PaneLayoutSizingPolicy(
  floors: [
    .editor: PaneLayoutSize(width: 575, height: 425),
    .agent: PaneLayoutSize(width: 370, height: 385),
    .terminal: PaneLayoutSize(width: 370, height: 331),
  ],
  gaps: PaneLayoutGaps(horizontal: 18, vertical: 18))

private let window = PaneLayoutSize(width: 1300, height: 800)

private func slotPane(_ id: String, _ mode: PaneMode) -> Pane {
  Pane(
    paneId: id, workspaceId: "workspace-1", name: id, cwd: "/repo", mode: mode, state: .idle,
    source: .protocol, confidence: 1, phrase: id, generation: 1, cols: 80, rows: 24,
    record: false)
}

private func leaf(_ id: String, _ kind: PaneLayoutKind) -> PaneLayoutBranch {
  PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: id, kind: kind)))
}

/// A workspace saved before tabs: one split per open file, then the agent.
@MainActor private func legacyWorkspace() throws -> (EngineClient, JsonValue) {
  let client = EngineClient(socket: "/tmp/editor-layout-slot.sock")
  for (id, mode) in [
    ("readme", PaneMode.editor), ("main", .editor), ("lib", .editor), ("agent", .agent),
    ("shell", .terminal),
  ] {
    client.upsertPane(slotPane(id, mode))
    if mode == .editor {
      client.upsertEditor(
        FileDocument(
          fileId: "file-\(id)", paneId: id, workspaceId: "workspace-1", path: "\(id).rs",
          language: .rust, revision: "1", diskRevision: "1", lineCount: 1, byteCount: "1",
          dirty: false, conflicted: false, watching: true))
    }
  }
  let legacy = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        leaf("readme", .editor), leaf("main", .editor), leaf("lib", .editor),
        PaneLayoutBranch(
          tree: .split(
            PaneLayoutSplit(
              axis: .vertical, children: [leaf("agent", .agent), leaf("shell", .terminal)]))),
      ]))
  return (client, try PaneLayoutState(tree: legacy, mode: .custom).encodedWorkspaceLayout())
}

@MainActor private func laidOut(_ client: EngineClient, _ stored: JsonValue) throws
  -> (state: PaneLayoutState, inserted: [String])
{
  let live = client.layoutPanes(client.panes)
  var state = PaneLayoutState(workspaceLayout: stored, livePanes: live)
  let inserted = try state.reconcile(livePanes: live, sizing: slotSizing)
  return (state, inserted)
}

@MainActor @Test func openFilesShareOneEditorAreaThatKeepsItsSlot() throws {
  let (client, stored) = try legacyWorkspace()
  client.activateEditorTab("main")

  let first = try laidOut(client, stored)
  let tree = try #require(first.state.tree)
  #expect(first.inserted.isEmpty)
  #expect(tree.paneIdsInCanonicalOrder == ["main", "agent", "shell"])
  #expect(tree.leavesInCanonicalOrder.filter { $0.kind == .editor }.count == 1)

  // Switching tabs puts the new active pane in the same slot; nothing moves.
  client.activateEditorTab("lib")
  let second = try laidOut(client, stored)
  #expect(second.inserted.isEmpty)
  #expect(second.state.tree?.paneIdsInCanonicalOrder == ["lib", "agent", "shell"])
  let before = try PaneLayoutGeometry.layout(tree: tree, viewport: window, sizing: slotSizing)
  let after = try PaneLayoutGeometry.layout(
    tree: try #require(second.state.tree), viewport: window, sizing: slotSizing)
  #expect(before.paneRects["main"] == after.paneRects["lib"])
  #expect(before.paneRects["agent"] == after.paneRects["agent"])

  // A layout repaired with the old active pane still resolves to the new one.
  var displayed = second.state
  let inserted = try displayed.reconcile(
    livePanes: client.layoutPanes(client.panes), sizing: slotSizing)
  #expect(inserted.isEmpty)
}

@MainActor @Test func everyPaneFrameStaysInsideTheWindow() throws {
  let (client, stored) = try legacyWorkspace()
  client.activateEditorTab("readme")
  let tree = try #require(try laidOut(client, stored).state.tree)
  let plan = try PaneLayoutGeometry.layout(tree: tree, viewport: window, sizing: slotSizing)
  #expect(plan.paneRects.count == 3)
  for (paneId, rect) in plan.paneRects {
    #expect(rect.origin.x >= 0 && rect.origin.y >= 0, "\(paneId)")
    #expect(rect.origin.x + rect.size.width <= window.width, "\(paneId)")
    #expect(rect.origin.y + rect.size.height <= window.height, "\(paneId)")
  }
  let editor = try #require(plan.paneRects["readme"])
  #expect(editor.size.width >= 575)

  // One split per file, as before tabs, could not fit and pushed panes out.
  let legacy = PaneLayoutState(workspaceLayout: stored)
  let split = try PaneLayoutGeometry.layout(
    tree: try #require(legacy.tree), viewport: window, sizing: slotSizing)
  #expect(
    split.paneRects.values.contains { $0.origin.x + $0.size.width > window.width })
}
