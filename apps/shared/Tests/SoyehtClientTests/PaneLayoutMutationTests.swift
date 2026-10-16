import Testing

@testable import SoyehtClient

private let mutationSizing = PaneLayoutSizingPolicy(
  floors: [
    .terminal: PaneLayoutSize(width: 300, height: 180),
    .agent: PaneLayoutSize(width: 280, height: 220),
    .editor: PaneLayoutSize(width: 600, height: 480),
    .web: PaneLayoutSize(width: 520, height: 420),
    .text: PaneLayoutSize(width: 317, height: 394),
  ],
  gaps: PaneLayoutGaps(horizontal: 20, vertical: 12))

@Test func splitTakesHalfOfItsAnchorAndFlattensTheSameAxis() throws {
  let tree = mutationSplit(
    .horizontal,
    (6, mutationLeaf("editor", .editor)),
    (2, mutationLeaf("terminal", .terminal)))

  let result = try PaneLayoutTreeMutation.splitting(
    tree, anchorPaneId: "editor", newLeaf: PaneLayoutLeaf(paneId: "web", kind: .web),
    axis: .horizontal)
  let split = try #require(result.mutationHorizontalSplit)

  #expect(
    split.children.map(\.tree.paneIdsInCanonicalOrder) == [["editor"], ["web"], ["terminal"]])
  #expect(split.children.map(\.weight) == [3, 3, 2])
  #expect(split.children.allSatisfy { $0.tree.mutationHorizontalSplit == nil })
}

@Test func splitOnAnotherAxisPreservesTheAnchorShareInItsParent() throws {
  let tree = mutationSplit(
    .horizontal,
    (6, mutationLeaf("editor", .editor)),
    (2, mutationLeaf("terminal", .terminal)))

  let result = try PaneLayoutTreeMutation.splitting(
    tree, anchorPaneId: "editor", newLeaf: PaneLayoutLeaf(paneId: "web", kind: .web),
    axis: .vertical)
  let root = try #require(result.mutationHorizontalSplit)
  let nested = try #require(root.children[0].tree.mutationVerticalSplit)

  #expect(root.children.map(\.weight) == [6, 2])
  #expect(nested.children.map(\.weight) == [1, 1])
  #expect(result.paneIdsInCanonicalOrder == ["editor", "web", "terminal"])
}

@Test func splitRejectsMissingAnchorAndDuplicatePane() throws {
  let tree = mutationLeaf("editor", .editor)

  #expect(throws: PaneLayoutMutationError.missingPane("missing")) {
    try PaneLayoutTreeMutation.splitting(
      tree, anchorPaneId: "missing", newLeaf: PaneLayoutLeaf(paneId: "web", kind: .web),
      axis: .horizontal)
  }
  #expect(throws: PaneLayoutMutationError.duplicatePane("editor")) {
    try PaneLayoutTreeMutation.splitting(
      tree, anchorPaneId: "editor", newLeaf: PaneLayoutLeaf(paneId: "editor", kind: .web),
      axis: .horizontal)
  }
}

@Test func edgeMovesInsertOnEveryRequestedSideAndPreserveEveryPane() throws {
  let tree = mutationSplit(
    .horizontal,
    (4, mutationLeaf("editor", .editor)),
    (2, mutationLeaf("terminal", .terminal)),
    (1, mutationLeaf("agent", .agent)))
  let expected: [(PaneLayoutDropZone, [String])] = [
    (.left, ["editor", "agent", "terminal"]),
    (.right, ["editor", "terminal", "agent"]),
    (.top, ["editor", "agent", "terminal"]),
    (.bottom, ["editor", "terminal", "agent"]),
  ]

  for (zone, order) in expected {
    let result = try PaneLayoutTreeMutation.moving(
      tree, paneId: "agent", relativeTo: "terminal", zone: zone)
    #expect(result.paneIdsInCanonicalOrder == order)
    #expect(Set(result.paneIdsInCanonicalOrder) == ["editor", "terminal", "agent"])
    if zone == .top || zone == .bottom {
      let root = try #require(result.mutationHorizontalSplit)
      #expect(root.children[1].tree.mutationVerticalSplit != nil)
    } else {
      let root = try #require(result.mutationHorizontalSplit)
      #expect(root.children.count == 3)
      #expect(root.children.allSatisfy { $0.tree.mutationHorizontalSplit == nil })
    }
  }
}

@Test func edgeMoveCollapsesTheSourceAndSplitsTheTargetShare() throws {
  let auxiliaries = mutationSplit(
    .vertical,
    (3, mutationLeaf("terminal", .terminal)),
    (1, mutationLeaf("agent", .agent)))
  let tree = mutationSplit(
    .horizontal,
    (5, mutationLeaf("editor", .editor)),
    (2, auxiliaries))

  let result = try PaneLayoutTreeMutation.moving(
    tree, paneId: "editor", relativeTo: "terminal", zone: .bottom)
  let root = try #require(result.mutationVerticalSplit)

  #expect(result.paneIdsInCanonicalOrder == ["terminal", "editor", "agent"])
  #expect(root.children.map(\.weight) == [1.5, 1.5, 1])
  #expect(root.children.allSatisfy { $0.tree.mutationVerticalSplit == nil })
}

@Test func centerDropSwapsLeavesWithoutChangingStructureOrWeights() throws {
  let right = mutationSplit(
    .vertical,
    (7, mutationLeaf("terminal", .terminal)),
    (5, mutationLeaf("agent", .agent)))
  let tree = mutationSplit(
    .horizontal,
    (3, mutationLeaf("editor", .editor)),
    (1, right))

  let result = try PaneLayoutTreeMutation.moving(
    tree, paneId: "editor", relativeTo: "agent", zone: .center)
  let root = try #require(result.mutationHorizontalSplit)
  let nested = try #require(root.children[1].tree.mutationVerticalSplit)

  #expect(result.paneIdsInCanonicalOrder == ["agent", "terminal", "editor"])
  #expect(root.children.map(\.weight) == [3, 1])
  #expect(nested.children.map(\.weight) == [7, 5])
  #expect(result.leavesInCanonicalOrder.map(\.kind) == [.agent, .terminal, .editor])
}

@Test func movingAPaneOntoItselfIsAnExplicitNoOp() throws {
  let tree = mutationSplit(
    .horizontal,
    (1, mutationLeaf("editor", .editor)),
    (1, mutationLeaf("terminal", .terminal)))

  for zone in PaneLayoutDropZone.allCases {
    #expect(
      try PaneLayoutTreeMutation.moving(
        tree, paneId: "editor", relativeTo: "editor", zone: zone) == tree)
  }
}

@Test func resizeChangesOnlyNeighborWeightsAndClampsAtMeasuredFloors() throws {
  let tree = mutationSplit(
    .horizontal,
    (4, mutationLeaf("editor", .editor)),
    (2, mutationLeaf("terminal", .terminal)),
    (3, mutationLeaf("agent", .agent)))
  let viewport = PaneLayoutSize(width: 1_500, height: 600)

  let moved = try PaneLayoutTreeMutation.resizing(
    tree, splitAt: PaneLayoutNodePath(), dividerIndex: 0, delta: 80,
    viewport: viewport, sizing: mutationSizing)
  let movedSplit = try #require(moved.mutationHorizontalSplit)
  #expect(movedSplit.children[0].weight > 4)
  #expect(movedSplit.children[1].weight < 2)
  #expect(movedSplit.children[2].weight == 3)
  #expect(abs(movedSplit.children[0].weight + movedSplit.children[1].weight - 6) < 0.000_001)

  let clamped = try PaneLayoutTreeMutation.resizing(
    tree, splitAt: PaneLayoutNodePath(), dividerIndex: 0, delta: 10_000,
    viewport: viewport, sizing: mutationSizing)
  let clampedSplit = try #require(clamped.mutationHorizontalSplit)
  let rects = try PaneLayoutGeometry.layout(
    tree: clamped, viewport: viewport, sizing: mutationSizing
  ).paneRects
  #expect(clampedSplit.children[1].weight > 0)
  #expect(abs(clampedSplit.children[0].weight + clampedSplit.children[1].weight - 6) < 0.000_001)
  #expect(abs(try #require(rects["terminal"]).size.width - 300) < 0.000_001)
}

@Test func resizeUsesNestedSplitPathAndPreservesItsProportionAcrossViewportChanges() throws {
  let nested = mutationSplit(
    .vertical,
    (1, mutationLeaf("terminal", .terminal)),
    (1, mutationLeaf("agent", .agent)))
  let tree = mutationSplit(
    .horizontal,
    (1, mutationLeaf("editor", .editor)),
    (1, nested))
  let path = PaneLayoutNodePath(components: [1])
  let resized = try PaneLayoutTreeMutation.resizing(
    tree, splitAt: path, dividerIndex: 0, delta: 40,
    viewport: PaneLayoutSize(width: 1_400, height: 800), sizing: mutationSizing)
  let split = try #require(resized.mutationHorizontalSplit?.children[1].tree.mutationVerticalSplit)
  let ratio = split.children[0].weight / split.children[1].weight

  let compact = try PaneLayoutGeometry.layout(
    tree: resized, viewport: PaneLayoutSize(width: 1_000, height: 650), sizing: mutationSizing)
  let wide = try PaneLayoutGeometry.layout(
    tree: resized, viewport: PaneLayoutSize(width: 1_800, height: 1_000), sizing: mutationSizing)
  let compactTerminal = try #require(compact.paneRects["terminal"])
  let compactAgent = try #require(compact.paneRects["agent"])
  let wideTerminal = try #require(wide.paneRects["terminal"])
  let wideAgent = try #require(wide.paneRects["agent"])
  let compactFlexibleRatio =
    (compactTerminal.size.height - 180) / (compactAgent.size.height - 220)
  let wideFlexibleRatio = (wideTerminal.size.height - 180) / (wideAgent.size.height - 220)

  #expect(ratio > 1)
  #expect(abs(compactFlexibleRatio - ratio) < 0.000_001)
  #expect(abs(wideFlexibleRatio - ratio) < 0.000_001)
  #expect(compact.nodes[path] != nil)
  #expect(wide.nodes[path] != nil)
}

@Test func resizeDoesNothingWhenTheSplitOnlyHasItsFloors() throws {
  let tree = mutationSplit(
    .horizontal,
    (1, mutationLeaf("editor", .editor)),
    (1, mutationLeaf("terminal", .terminal)))
  let result = try PaneLayoutTreeMutation.resizing(
    tree, splitAt: PaneLayoutNodePath(), dividerIndex: 0, delta: 100,
    viewport: PaneLayoutSize(width: 500, height: 400), sizing: mutationSizing)

  #expect(result == tree)
}

@Test func companionResizeUsesPresentedAxisAndKeepsThePersistedStructure() throws {
  let companion = PaneLayoutCompanion(writingPaneId: "writing", agentPaneId: "agent")
  let tree = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(tree: mutationLeaf("writing", .text)),
        PaneLayoutBranch(tree: mutationLeaf("agent", .agent)),
      ],
      companion: companion))

  let resized = try PaneLayoutTreeMutation.resizingPresentedCompanion(
    tree, splitAt: PaneLayoutNodePath(), presentationAxis: .vertical,
    dividerIndex: 0, delta: 50,
    viewport: PaneLayoutSize(width: 500, height: 1_000), sizing: mutationSizing)
  let split = try #require(
    {
      if case .split(let split) = resized { return split }
      return nil
    }())

  #expect(split.axis == .horizontal)
  #expect(split.companion == companion)
  #expect(split.children[0].weight > split.children[1].weight)
  #expect(abs(split.children[0].weight + split.children[1].weight - 2) < 0.000_001)

  let equalized = try PaneLayoutTreeMutation.equalizingPresentedCompanion(
    resized, splitAt: PaneLayoutNodePath(), presentationAxis: .vertical,
    viewport: PaneLayoutSize(width: 500, height: 1_000), sizing: mutationSizing)
  let equalizedSplit = try #require(
    {
      if case .split(let split) = equalized { return split }
      return nil
    }())
  #expect(equalizedSplit.axis == .horizontal)
  #expect(equalizedSplit.companion == companion)
}

@Test func movingCompanionMembersAgainstEachOtherKeepsTheExplicitLink() throws {
  let pair = try PaneLayoutTreeMutation.splittingWritingCompanion(
    .leaf(PaneLayoutLeaf(paneId: "writing", kind: .text)),
    writingPaneId: "writing",
    agentLeaf: PaneLayoutLeaf(paneId: "agent", kind: .agent),
    axis: .horizontal)

  let moved = try PaneLayoutTreeMutation.moving(
    pair, paneId: "agent", relativeTo: "writing", zone: .top)
  guard case .split(let split) = moved else {
    Issue.record("Expected the companion split")
    return
  }

  #expect(split.axis == .vertical)
  #expect(split.children.map(\.tree.paneIdsInCanonicalOrder) == [["agent"], ["writing"]])
  #expect(
    split.companion
      == PaneLayoutCompanion(writingPaneId: "writing", agentPaneId: "agent"))
}

@Test func equalizeChangesOnlyTheRequestedSplitsDirectChildren() throws {
  let nested = mutationSplit(
    .vertical,
    (7, mutationLeaf("terminal", .terminal)),
    (5, mutationLeaf("agent", .agent)))
  let tree = mutationSplit(
    .horizontal,
    (3, mutationLeaf("editor", .editor)),
    (1, nested))

  let viewport = PaneLayoutSize(width: 1_400, height: 700)
  let result = try PaneLayoutTreeMutation.equalizing(
    tree, splitAt: PaneLayoutNodePath(components: [1]), viewport: viewport,
    sizing: mutationSizing)
  let root = try #require(result.mutationHorizontalSplit)
  let equalized = try #require(root.children[1].tree.mutationVerticalSplit)
  let rects = try PaneLayoutGeometry.layout(
    tree: result, viewport: viewport, sizing: mutationSizing
  ).paneRects

  #expect(root.children.map(\.weight) == [3, 1])
  #expect(equalized.children.map(\.weight) == [164, 124])
  #expect(
    abs(
      (try #require(rects["terminal"])).size.height
        - (try #require(rects["agent"])).size.height) < 0.000_001)
}

@Test func equalizeLetsAnOversizedFloorWinAndSharesTheRemainder() throws {
  let sizing = PaneLayoutSizingPolicy(
    floors: [
      .editor: PaneLayoutSize(width: 600, height: 480),
      .terminal: PaneLayoutSize(width: 200, height: 180),
      .agent: PaneLayoutSize(width: 200, height: 220),
    ],
    gaps: PaneLayoutGaps(horizontal: 20, vertical: 12))
  let tree = mutationSplit(
    .horizontal,
    (7, mutationLeaf("editor", .editor)),
    (2, mutationLeaf("terminal", .terminal)),
    (1, mutationLeaf("agent", .agent)))
  let viewport = PaneLayoutSize(width: 1_200, height: 600)

  let result = try PaneLayoutTreeMutation.equalizing(
    tree, splitAt: PaneLayoutNodePath(), viewport: viewport, sizing: sizing)
  let rects = try PaneLayoutGeometry.layout(
    tree: result, viewport: viewport, sizing: sizing
  ).paneRects

  #expect(abs((try #require(rects["editor"])).size.width - 600) < 0.000_001)
  #expect(abs((try #require(rects["terminal"])).size.width - 280) < 0.000_001)
  #expect(abs((try #require(rects["agent"])).size.width - 280) < 0.000_001)
}

@Test func mutationRejectsInvalidPathsDividersAndDeltas() throws {
  let tree = mutationSplit(
    .horizontal,
    (1, mutationLeaf("editor", .editor)),
    (1, mutationLeaf("terminal", .terminal)))

  #expect(throws: PaneLayoutMutationError.invalidSplitPath(PaneLayoutNodePath(components: [0]))) {
    try PaneLayoutTreeMutation.equalizing(
      tree, splitAt: PaneLayoutNodePath(components: [0]),
      viewport: PaneLayoutSize(width: 1_000, height: 600), sizing: mutationSizing)
  }
  #expect(throws: PaneLayoutMutationError.invalidDivider(1)) {
    try PaneLayoutTreeMutation.resizing(
      tree, splitAt: PaneLayoutNodePath(), dividerIndex: 1, delta: 1,
      viewport: PaneLayoutSize(width: 1_000, height: 600), sizing: mutationSizing)
  }
  #expect(throws: PaneLayoutMutationError.invalidDelta) {
    try PaneLayoutTreeMutation.resizing(
      tree, splitAt: PaneLayoutNodePath(), dividerIndex: 0, delta: .nan,
      viewport: PaneLayoutSize(width: 1_000, height: 600), sizing: mutationSizing)
  }
}

private func mutationLeaf(_ paneId: String, _ kind: PaneLayoutKind) -> PaneLayoutTree {
  .leaf(PaneLayoutLeaf(paneId: paneId, kind: kind))
}

private func mutationSplit(
  _ axis: PaneLayoutAxis, _ children: (Double, PaneLayoutTree)...
) -> PaneLayoutTree {
  .split(
    PaneLayoutSplit(
      axis: axis,
      children: children.map { PaneLayoutBranch(weight: $0.0, tree: $0.1) }))
}

extension PaneLayoutTree {
  fileprivate var mutationHorizontalSplit: PaneLayoutSplit? {
    guard case .split(let split) = self, split.axis == .horizontal else { return nil }
    return split
  }

  fileprivate var mutationVerticalSplit: PaneLayoutSplit? {
    guard case .split(let split) = self, split.axis == .vertical else { return nil }
    return split
  }
}
