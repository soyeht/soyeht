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
  #expect(try encoded.decoded(WorkspacePaneLayoutEnvelope.self).version == 2)
}

@Test func versionOnePreservesItsTreeAndMigratesWithZeroCompanions() throws {
  let versionOne = JsonValue.object([
    "version": .integer(1),
    "mode": .string("custom"),
    "tree": .object([
      "axis": .string("horizontal"),
      "children": .array([
        .object([
          "weight": .integer(2),
          "node": .object(["paneId": .string("writing"), "kind": .string("text")]),
        ]),
        .object([
          "weight": .integer(1),
          "node": .object(["paneId": .string("agent"), "kind": .string("agent")]),
        ]),
      ]),
      "companion": .object([
        "writingPaneId": .string("writing"), "agentPaneId": .string("agent"),
      ]),
    ]),
  ])

  let state = PaneLayoutState(workspaceLayout: versionOne)
  let encoded = try state.encodedWorkspaceLayout()
  let envelope = try encoded.decoded(WorkspacePaneLayoutEnvelope.self)

  #expect(envelope.version == 2)
  #expect(state.tree?.paneIdsInCanonicalOrder == ["writing", "agent"])
  guard case .split(let split) = state.tree else {
    Issue.record("The v1 tree did not survive migration")
    return
  }
  #expect(split.children.map(\.weight) == [2, 1])
  #expect(split.companion == nil)
}

@Test func versionTwoRoundTripsAnExplicitCompanionOnItsSubtree() throws {
  let companion = PaneLayoutCompanion(writingPaneId: "writing", agentPaneId: "agent")
  let state = PaneLayoutState(
    tree: .split(
      PaneLayoutSplit(
        axis: .horizontal,
        children: [
          PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "writing", kind: .text))),
          PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "agent", kind: .agent))),
        ],
        companion: companion)),
    mode: .custom)

  #expect(PaneLayoutState(workspaceLayout: try state.encodedWorkspaceLayout()) == state)
}

@Test func companionRequiresExactlyTwoDirectLeafChildren() {
  let companion: JsonValue = .object([
    "writingPaneId": .string("writing"), "agentPaneId": .string("agent"),
  ])
  let writing: JsonValue = .object(["paneId": .string("writing"), "kind": .string("text")])
  let agent: JsonValue = .object(["paneId": .string("agent"), "kind": .string("agent")])
  let terminal: JsonValue = .object([
    "paneId": .string("terminal"), "kind": .string("terminal"),
  ])
  let branch: (JsonValue) -> JsonValue = { node in
    .object(["weight": .integer(1), "node": node])
  }
  let extraChild: JsonValue = .object([
    "version": .integer(2), "mode": .string("custom"),
    "tree": .object([
      "axis": .string("horizontal"),
      "children": .array([branch(writing), branch(agent), branch(terminal)]),
      "companion": companion,
    ]),
  ])
  let nestedMember: JsonValue = .object([
    "version": .integer(2), "mode": .string("custom"),
    "tree": .object([
      "axis": .string("horizontal"),
      "children": .array([
        branch(writing),
        branch(
          .object([
            "axis": .string("vertical"),
            "children": .array([branch(agent), branch(terminal)]),
          ])),
      ]),
      "companion": companion,
    ]),
  ])

  #expect(PaneLayoutState(workspaceLayout: extraChild).tree == nil)
  #expect(PaneLayoutState(workspaceLayout: nestedMember).tree == nil)
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
