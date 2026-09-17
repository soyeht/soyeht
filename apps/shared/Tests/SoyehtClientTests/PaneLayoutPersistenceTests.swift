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
