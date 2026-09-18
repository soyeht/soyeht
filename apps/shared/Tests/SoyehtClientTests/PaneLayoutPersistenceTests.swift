import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func absentWorkspaceLayoutMigratesToInitialByType() {
  let state = PaneLayoutState(workspaceLayout: .object(["panes": .array([])]))

  #expect(state.tree == nil)
  #expect(state.mode == .automatic)
  #expect(state.insertionRule == .initialByType)
}

@Test func unknownWorkspaceLayoutVersionFallsBackWithoutThrowing() {
  let state = PaneLayoutState(
    workspaceLayout: .object([
      "version": .integer(99),
      "mode": .string("custom"),
      "tree": .object([
        "paneId": .string("pane"),
        "kind": .string("terminal"),
      ]),
    ]))

  #expect(state.tree == nil)
  #expect(state.mode == .automatic)
  #expect(state.insertionRule == .initialByType)
}

@Test func firstDragPersistsTreeAndCustomModeInOneEnvelope() throws {
  let tree = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(
          tree: .leaf(PaneLayoutLeaf(paneId: "editor", kind: .editor))),
        PaneLayoutBranch(
          tree: .leaf(PaneLayoutLeaf(paneId: "terminal", kind: .terminal))),
      ]))
  var state = PaneLayoutState(tree: tree)

  state.recordUserArrangement(tree)
  let encoded = try state.encodedWorkspaceLayout()
  let restored = PaneLayoutState(workspaceLayout: encoded)

  #expect(restored == state)
  #expect(restored.insertionRule == .preservingUserArrangement)
  #expect(try encoded.decoded(WorkspacePaneLayoutEnvelope.self).version == 1)
}

@Test func invalidPersistedTreeFallsBackToAutomaticLayout() {
  let duplicate = JsonValue.object([
    "version": .integer(1),
    "mode": .string("custom"),
    "tree": .object([
      "axis": .string("horizontal"),
      "children": .array([
        .object([
          "weight": .integer(1),
          "node": .object([
            "paneId": .string("same"), "kind": .string("terminal"),
          ]),
        ]),
        .object([
          "weight": .integer(1),
          "node": .object([
            "paneId": .string("same"), "kind": .string("terminal"),
          ]),
        ]),
      ]),
    ]),
  ])

  #expect(PaneLayoutState(workspaceLayout: duplicate) == PaneLayoutState())
}

@Test func loadingLayoutPrunesClosedPaneAndCollapsesItsSplit() throws {
  let persisted = JsonValue.object([
    "version": .integer(1),
    "mode": .string("custom"),
    "tree": .object([
      "axis": .string("horizontal"),
      "children": .array([
        .object([
          "weight": .integer(2),
          "node": .object([
            "paneId": .string("closed"), "kind": .string("terminal"),
          ]),
        ]),
        .object([
          "weight": .integer(1),
          "node": .object([
            "paneId": .string("open"), "kind": .string("terminal"),
          ]),
        ]),
      ]),
    ]),
  ])

  let state = PaneLayoutState(workspaceLayout: persisted, livePanes: [terminalPane("open")])

  #expect(state.mode == .custom)
  #expect(
    state.tree == .leaf(PaneLayoutLeaf(paneId: "open", kind: .terminal)))
}

@Test func loadingCustomLayoutKeepsItWhenALivePaneIsMissingFromTheTree() throws {
  let persisted = try PaneLayoutState(
    tree: .leaf(PaneLayoutLeaf(paneId: "editor", kind: .editor)), mode: .custom
  ).encodedWorkspaceLayout()
  let editor = Pane(
    paneId: "editor", workspaceId: "workspace", name: "Editor", cwd: "/workspace",
    mode: .editor, state: .idle, source: .protocol, confidence: 1, phrase: "Ready",
    generation: 1, cols: 80, rows: 24, record: false)
  let terminal = terminalPane("terminal")
  var state = PaneLayoutState(workspaceLayout: persisted, livePanes: [editor, terminal])
  let sizing = PaneLayoutSizingPolicy(
    floors: [
      .editor: PaneLayoutSize(width: 600, height: 480),
      .terminal: PaneLayoutSize(width: 320, height: 180),
    ],
    gaps: PaneLayoutGaps(horizontal: 18, vertical: 18))

  let inserted = try state.reconcile(livePanes: [editor, terminal], sizing: sizing)

  #expect(inserted == ["terminal"])
  #expect(state.mode == .custom)
  #expect(state.insertionRule == .preservingUserArrangement)
  #expect(state.tree?.paneIdsInCanonicalOrder == ["editor", "terminal"])
}

private func terminalPane(_ paneId: String) -> Pane {
  Pane(
    paneId: paneId, workspaceId: "workspace", name: paneId, cwd: "/workspace", mode: .terminal,
    state: .idle, source: .protocol, confidence: 1, phrase: "Ready", generation: 1, cols: 80,
    rows: 24, record: false)
}
