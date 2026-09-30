import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

@Test func layoutFailureUsesAFixedUserMessageInsteadOfTheTransportError() {
  let failure = PaneLayoutCommitFailure(workspaceId: "workspace", message: "unauthorized: 401")

  #expect(
    PaneLayoutCommitFailure.userMessage
      == "Couldn't save the layout. The last saved arrangement is back.")
  #expect(!PaneLayoutCommitFailure.userMessage.contains(failure.message))
}

@Test @MainActor
func gestureDraftIsLocalUntilCommitAndThenBecomesCustom() async throws {
  let probe = PaneLayoutPersistProbe()
  let controller = PaneLayoutController(persist: probe.persist)
  let initial = horizontalTree("a", "b")
  let moved = verticalTree("b", "a")
  controller.synchronize(
    workspaceId: "workspace", confirmed: PaneLayoutState(tree: initial, mode: .automatic))

  #expect(try controller.beginDraft(workspaceId: "workspace") == initial)
  try controller.updateDraft(workspaceId: "workspace", tree: moved)
  #expect(controller.presentation(for: "workspace")?.displayed.tree == moved)
  #expect(controller.presentation(for: "workspace")?.confirmed.tree == initial)
  #expect(probe.requests.isEmpty)

  controller.cancelDraft(workspaceId: "workspace")
  #expect(controller.presentation(for: "workspace")?.displayed.tree == initial)

  _ = try controller.beginDraft(workspaceId: "workspace")
  try controller.updateDraft(workspaceId: "workspace", tree: moved)
  try controller.commitDraft(workspaceId: "workspace")
  await expectEventually { probe.requests.count == 1 }

  #expect(probe.requests[0].state.tree == moved)
  #expect(probe.requests[0].state.mode == .custom)
  #expect(controller.presentation(for: "workspace")?.displayed.tree == moved)
  #expect(controller.presentation(for: "workspace")?.confirmed.tree == initial)

  probe.succeedNext()
  await expectEventually { controller.presentation(for: "workspace")?.isPersisting == false }
  #expect(controller.presentation(for: "workspace")?.confirmed == probe.requests[0].state)
}

@Test @MainActor
func commitsAreSerializedAndAnOlderReplyNeverBeatsANewerDraft() async throws {
  let probe = PaneLayoutPersistProbe()
  let controller = PaneLayoutController(persist: probe.persist)
  let initial = horizontalTree("a", "b")
  let first = verticalTree("a", "b")
  let second = verticalTree("b", "a")
  let newestDraft = weightedHorizontalTree("b", "a")
  controller.synchronize(
    workspaceId: "workspace", confirmed: PaneLayoutState(tree: initial, mode: .automatic))

  try controller.commit(tree: first, workspaceId: "workspace")
  await expectEventually { probe.requests.count == 1 }
  try controller.commit(tree: second, workspaceId: "workspace")
  await Task.yield()

  #expect(probe.requests.count == 1)
  #expect(controller.presentation(for: "workspace")?.pendingCommitCount == 2)
  #expect(controller.presentation(for: "workspace")?.displayed.tree == second)

  probe.succeedNext()
  await expectEventually { probe.requests.count == 2 }
  #expect(controller.presentation(for: "workspace")?.confirmed.tree == first)
  #expect(controller.presentation(for: "workspace")?.displayed.tree == second)

  _ = try controller.beginDraft(workspaceId: "workspace")
  try controller.updateDraft(workspaceId: "workspace", tree: newestDraft)
  probe.succeedNext()
  await expectEventually { controller.presentation(for: "workspace")?.isPersisting == false }

  let withDraft = try #require(controller.presentation(for: "workspace"))
  #expect(withDraft.confirmed.tree == second)
  #expect(withDraft.displayed.tree == newestDraft)
  #expect(withDraft.draftTree == newestDraft)

  controller.cancelDraft(workspaceId: "workspace")
  #expect(controller.presentation(for: "workspace")?.displayed.tree == second)
}

@Test @MainActor
func aFailedCommitDropsDependentStatesAndRestoresTheLatestConfirmation() async throws {
  let probe = PaneLayoutPersistProbe()
  let controller = PaneLayoutController(persist: probe.persist)
  let initial = horizontalTree("a", "b")
  let confirmed = verticalTree("a", "b")
  let failing = verticalTree("b", "a")
  let dependent = weightedHorizontalTree("a", "b")
  controller.synchronize(
    workspaceId: "workspace", confirmed: PaneLayoutState(tree: initial, mode: .automatic))

  try controller.commit(tree: confirmed, workspaceId: "workspace")
  await expectEventually { probe.requests.count == 1 }
  probe.succeedNext()
  await expectEventually { controller.presentation(for: "workspace")?.isPersisting == false }

  try controller.commit(tree: failing, workspaceId: "workspace")
  await expectEventually { probe.requests.count == 2 }
  try controller.commit(tree: dependent, workspaceId: "workspace")
  probe.failNext(PaneLayoutProbeError.refused)
  await expectEventually { controller.presentation(for: "workspace")?.isPersisting == false }

  let restored = try #require(controller.presentation(for: "workspace"))
  #expect(restored.confirmed.tree == confirmed)
  #expect(restored.displayed.tree == confirmed)
  #expect(restored.pendingCommitCount == 0)
  #expect(
    controller.failure
      == PaneLayoutCommitFailure(
        workspaceId: "workspace", message: "layout refused"))
  #expect(probe.requests.count == 2)
}

@Test @MainActor
func aPaneCreatedBeforeLayoutFailureRemainsVisibleThroughReconciliation() async throws {
  let probe = PaneLayoutPersistProbe()
  let controller = PaneLayoutController(persist: probe.persist)
  let initial = PaneLayoutTree.leaf(PaneLayoutLeaf(paneId: "anchor", kind: .terminal))
  let split = try PaneLayoutTreeMutation.splitting(
    initial, anchorPaneId: "anchor",
    newLeaf: PaneLayoutLeaf(paneId: "created", kind: .terminal), axis: .horizontal)
  controller.synchronize(
    workspaceId: "workspace", confirmed: PaneLayoutState(tree: initial, mode: .custom))

  try controller.commit(tree: split, workspaceId: "workspace")
  await expectEventually { probe.requests.count == 1 }
  probe.failNext(PaneLayoutProbeError.refused)
  await expectEventually { controller.presentation(for: "workspace")?.isPersisting == false }
  #expect(controller.presentation(for: "workspace")?.displayed.tree == initial)

  let reconciled = try controller.reconciledPresentation(
    for: "workspace", livePanes: [terminalPane("anchor"), terminalPane("created")],
    sizing: testSizing)
  #expect(reconciled.insertedPaneIds == ["created"])
  #expect(reconciled.state.mode == .custom)
  #expect(Set(reconciled.state.tree?.paneIdsInCanonicalOrder ?? []) == ["anchor", "created"])
}

@Test @MainActor
func firstSplitUsesTheReconciledLiveTreeWhenNoLayoutWasStoredYet() async throws {
  let probe = PaneLayoutPersistProbe()
  let controller = PaneLayoutController(persist: probe.persist)
  controller.synchronize(
    workspaceId: "workspace", confirmed: PaneLayoutState(tree: nil, mode: .automatic))

  let reconciled = try controller.reconciledPresentation(
    for: "workspace", livePanes: [terminalPane("anchor"), terminalPane("created")],
    sizing: testSizing)
  let displayed = try #require(reconciled.state.tree)
  let base = try #require(displayed.removingPane(withId: "created"))
  let split = try PaneLayoutTreeMutation.splitting(
    base, anchorPaneId: "anchor",
    newLeaf: PaneLayoutLeaf(paneId: "created", kind: .terminal), axis: .horizontal)

  try controller.commit(tree: split, workspaceId: "workspace")
  await expectEventually { probe.requests.count == 1 }
  #expect(probe.requests[0].state.tree == split)
  #expect(probe.requests[0].state.mode == .custom)
}

@Test @MainActor
func anAuthoritativeUpdateChangesConfirmationButNotQueuedPresentation() async throws {
  let probe = PaneLayoutPersistProbe()
  let controller = PaneLayoutController(persist: probe.persist)
  let initial = horizontalTree("a", "b")
  let queued = verticalTree("b", "a")
  let serverEvent = verticalTree("a", "b")
  controller.synchronize(
    workspaceId: "workspace", confirmed: PaneLayoutState(tree: initial, mode: .automatic))

  try controller.commit(tree: queued, workspaceId: "workspace")
  await expectEventually { probe.requests.count == 1 }
  controller.synchronize(
    workspaceId: "workspace", confirmed: PaneLayoutState(tree: serverEvent, mode: .custom))

  let duringRequest = try #require(controller.presentation(for: "workspace"))
  #expect(duringRequest.confirmed.tree == serverEvent)
  #expect(duringRequest.displayed.tree == queued)

  probe.succeedNext()
  await expectEventually { controller.presentation(for: "workspace")?.isPersisting == false }
  #expect(controller.presentation(for: "workspace")?.confirmed.tree == queued)
}

@Test @MainActor
func responseFromARemovedWorkspaceCannotStopItsReplacementWorker() async throws {
  let probe = PaneLayoutPersistProbe()
  let controller = PaneLayoutController(persist: probe.persist)
  let initial = horizontalTree("a", "b")
  let abandoned = verticalTree("a", "b")
  let replacement = verticalTree("b", "a")
  controller.synchronize(
    workspaceId: "workspace", confirmed: PaneLayoutState(tree: initial, mode: .automatic))

  try controller.commit(tree: abandoned, workspaceId: "workspace")
  await expectEventually { probe.requests.count == 1 }
  controller.remove(workspaceId: "workspace")
  controller.synchronize(
    workspaceId: "workspace", confirmed: PaneLayoutState(tree: initial, mode: .automatic))
  try controller.commit(tree: replacement, workspaceId: "workspace")
  await expectEventually { probe.requests.count == 2 }

  probe.succeedNext()
  await Task.yield()
  let whileReplacementPersists = try #require(controller.presentation(for: "workspace"))
  #expect(whileReplacementPersists.isPersisting)
  #expect(whileReplacementPersists.displayed.tree == replacement)
  #expect(whileReplacementPersists.pendingCommitCount == 1)

  probe.succeedNext()
  await expectEventually { controller.presentation(for: "workspace")?.isPersisting == false }
  #expect(controller.presentation(for: "workspace")?.confirmed.tree == replacement)
}

@MainActor
private final class PaneLayoutPersistProbe {
  struct Request {
    var workspaceId: String
    var state: PaneLayoutState
  }

  private(set) var requests: [Request] = []
  private var continuations: [CheckedContinuation<PaneLayoutState, Error>] = []

  func persist(workspaceId: String, state: PaneLayoutState) async throws -> PaneLayoutState {
    requests.append(Request(workspaceId: workspaceId, state: state))
    return try await withCheckedThrowingContinuation { continuation in
      continuations.append(continuation)
    }
  }

  func succeedNext() {
    let continuation = continuations.removeFirst()
    let requestIndex = requests.count - continuations.count - 1
    continuation.resume(returning: requests[requestIndex].state)
  }

  func failNext(_ error: Error) {
    continuations.removeFirst().resume(throwing: error)
  }
}

private enum PaneLayoutProbeError: LocalizedError {
  case refused

  var errorDescription: String? { "layout refused" }
}

@MainActor
private func expectEventually(
  attempts: Int = 1_000, _ condition: @MainActor () -> Bool,
  sourceLocation: SourceLocation = #_sourceLocation
) async {
  for _ in 0..<attempts {
    if condition() { return }
    await Task.yield()
  }
  Issue.record("Condition did not become true", sourceLocation: sourceLocation)
}

private func horizontalTree(_ first: String, _ second: String) -> PaneLayoutTree {
  splitTree(axis: .horizontal, first: first, second: second)
}

private func verticalTree(_ first: String, _ second: String) -> PaneLayoutTree {
  splitTree(axis: .vertical, first: first, second: second)
}

private func weightedHorizontalTree(_ first: String, _ second: String) -> PaneLayoutTree {
  .split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(
          weight: 2, tree: .leaf(PaneLayoutLeaf(paneId: first, kind: .terminal))),
        PaneLayoutBranch(
          weight: 1, tree: .leaf(PaneLayoutLeaf(paneId: second, kind: .terminal))),
      ]))
}

private func splitTree(
  axis: PaneLayoutAxis, first: String, second: String
) -> PaneLayoutTree {
  .split(
    PaneLayoutSplit(
      axis: axis,
      children: [
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: first, kind: .terminal))),
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: second, kind: .terminal))),
      ]))
}

private var testSizing: PaneLayoutSizingPolicy {
  PaneLayoutSizingPolicy(
    floors: [.terminal: PaneLayoutSize(width: 300, height: 180)],
    gaps: PaneLayoutGaps(horizontal: 18, vertical: 18))
}

private func terminalPane(_ paneId: String) -> Pane {
  Pane(
    paneId: paneId, workspaceId: "workspace", name: paneId, cwd: "/workspace", mode: .terminal,
    state: .idle, source: .protocol, confidence: 1, phrase: "Ready", generation: 1, cols: 80,
    rows: 24, record: false)
}
