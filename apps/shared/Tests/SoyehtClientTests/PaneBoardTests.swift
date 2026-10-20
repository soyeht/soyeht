import Foundation
import SoyehtProtocol
import SwiftTerm
import Testing

@testable import SoyehtClient

private func viewport(_ width: Double, _ height: Double) -> PaneLayoutSize {
  PaneLayoutSize(width: width, height: height)
}

private func boardOf(
  _ kinds: [PaneLayoutKind], in size: PaneLayoutSize,
  projectPanel: EditorProjectPanelPreference = .automatic
) throws -> PaneBoard {
  try PaneBoardPolicy.board(
    tree: try PaneLayoutFixture.workspace(kinds), viewport: size, sizing: PaneLayoutFixture.sizing,
    projectPanel: projectPanel)
}

private func right(of a: PaneBoardPlacement, is b: PaneBoardPlacement) -> Bool {
  b.rect.origin.x >= a.rect.origin.x + a.rect.size.width
}

@Test func twoPanesThatFitSitSideBySideInTheStoredArrangement() throws {
  // A terminal floor is 340 wide and an agent floor 340: both fit in 800 with the 18 point gap.
  let board = try boardOf([.web, .terminal], in: viewport(800, 700))

  #expect(board.keepsArrangement)
  #expect(board.tabs == nil)
  #expect(board.paneIds == ["web-0", "terminal-1"])
  let placements = board.placements
  #expect(placements.count == 2)
  #expect(placements[0].rect.origin.y == placements[1].rect.origin.y)
  #expect(right(of: placements[0], is: placements[1]))
  #expect(board.overflowAxes(in: viewport(800, 700)).isEmpty)
}

@Test func theBoardUsesTheSharedFitRules() throws {
  let tree = try PaneLayoutFixture.workspace([.terminal, .agent, .terminal])
  for size in [viewport(1100, 800), viewport(720, 800), viewport(500, 800), viewport(360, 500)] {
    let fit = try PaneLayoutFit.layout(
      tree: tree, viewport: size, sizing: PaneLayoutFixture.sizing)
    let board = try PaneBoardPolicy.board(
      tree: tree, viewport: size, sizing: PaneLayoutFixture.sizing)

    #expect(board.keepsArrangement == fit.keepsArrangement)
    #expect(board.tabs?.paneIds == fit.plan.tabGroup?.paneIds)
    #expect(Set(board.paneIds) == Set(tree.paneIdsInCanonicalOrder))
    for placement in board.placements {
      #expect(fit.plan.paneRects[placement.paneId] == placement.rect)
    }
  }
}

@Test func panesThatDoNotFitSideBySideStackBeforeTheyTab() throws {
  // 500 is narrower than two 340 floors, but 700 holds a terminal over an agent.
  let board = try boardOf([.web, .terminal], in: viewport(500, 700))

  #expect(!board.keepsArrangement)
  #expect(board.tabs == nil)
  let placements = board.placements
  #expect(placements.count == 2)
  #expect(placements[0].rect.origin.x == placements[1].rect.origin.x)
  #expect(
    placements[1].rect.origin.y >= placements[0].rect.origin.y + placements[0].rect.size.height)
}

@Test func panesThatFitNeitherWayShareOneTabGroup() throws {
  let size = viewport(500, 400)
  let board = try boardOf([.terminal, .agent], in: size)

  #expect(board.placements.isEmpty)
  let tabs = try #require(board.tabs)
  #expect(tabs.paneIds == ["terminal-0", "agent-1"])
  #expect(board.paneIds == tabs.paneIds)
  #expect(tabs.area.origin.y == tabs.strip.origin.y + tabs.strip.size.height)
  #expect(board.overflowAxes(in: size).isEmpty)
}

@Test func aViewportSmallerThanOnePaneScrollsInsteadOfClipping() throws {
  let size = viewport(300, 200)
  let board = try boardOf([.terminal, .agent], in: size)

  #expect(board.overflowAxes(in: size) == [.horizontal, .vertical])
}

@Test func theViewportIsTheAreaMinusTheMarginOnEverySide() {
  let inner = PaneBoardPolicy.viewport(in: viewport(900, 600))

  #expect(inner.width == 900 - 2 * PaneLayoutSpacing.outerMargin)
  #expect(inner.height == 600 - 2 * PaneLayoutSpacing.outerMargin)
  #expect(PaneBoardPolicy.viewport(in: viewport(10, 10)) == viewport(0, 0))
}

@Test func panesTheEngineNoLongerHoldsLeaveTheVisibleTree() throws {
  let tree = try PaneLayoutFixture.workspace([.terminal, .agent, .terminal])

  let visible = PaneBoardPolicy.visibleTree(tree, livePaneIds: ["terminal-0", "terminal-2"])

  #expect(visible?.paneIdsInCanonicalOrder.sorted() == ["terminal-0", "terminal-2"])
  #expect(PaneBoardPolicy.visibleTree(tree, livePaneIds: []) == nil)
}

// MARK: Focus

@Test func focusStaysOnAPaneOfTheBoardAndOtherwiseFallsBack() throws {
  let board = try boardOf([.terminal, .agent], in: viewport(800, 700))

  #expect(board.reconciledFocus("agent-1", preferred: "terminal-0") == "agent-1")
  #expect(board.reconciledFocus("gone", preferred: "agent-1") == "agent-1")
  #expect(board.reconciledFocus(nil, preferred: "gone") == board.paneIds.first)
  #expect(board.reconciledFocus(nil, preferred: nil) == board.paneIds.first)
}

@Test func theKeysStepThroughTheBoardInVisualOrderAndWrap() throws {
  let board = try boardOf([.terminal, .agent, .terminal], in: viewport(1200, 700))
  let order = board.paneIds

  #expect(board.focus(after: order[0], by: 1) == order[1])
  #expect(board.focus(after: order[2], by: 1) == order[0])
  #expect(board.focus(after: order[0], by: -1) == order[2])
  #expect(board.focus(after: nil, by: 1) == order[0])
  #expect(board.focus(after: nil, by: -1) == order[2])
  #expect(board.focus(after: "gone", by: 1) == order[0])
}

@Test func theArrowsMoveToThePaneBesideAndStopAtTheEdge() throws {
  let board = try boardOf([.web, .terminal], in: viewport(800, 700))

  #expect(board.neighbor(of: "web-0", toward: .right, shown: nil) == "terminal-1")
  #expect(board.neighbor(of: "terminal-1", toward: .left, shown: nil) == "web-0")
  #expect(board.neighbor(of: "web-0", toward: .left, shown: nil) == nil)
  #expect(board.neighbor(of: "web-0", toward: .down, shown: nil) == nil)
  #expect(board.neighbor(of: "gone", toward: .right, shown: nil) == nil)
}

@Test func theArrowsMoveVerticallyInAStackedBoard() throws {
  let board = try boardOf([.web, .terminal], in: viewport(500, 700))

  #expect(board.neighbor(of: "web-0", toward: .down, shown: nil) == "terminal-1")
  #expect(board.neighbor(of: "terminal-1", toward: .up, shown: nil) == "web-0")
  #expect(board.neighbor(of: "web-0", toward: .right, shown: nil) == nil)
}

@Test func anArrowChoosesTheNearestPaneThatSharesTheFocusedExtent() throws {
  // An agent beside a terminal stacked over a second terminal on the right.
  let tree = PaneLayoutTree.split(
    PaneLayoutSplit(
      axis: .horizontal,
      children: [
        PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "left", kind: .agent))),
        PaneLayoutBranch(
          tree: .split(
            PaneLayoutSplit(
              axis: .vertical,
              children: [
                PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "top", kind: .terminal))),
                PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "bottom", kind: .terminal))),
              ]))),
      ]))
  let board = try PaneBoardPolicy.board(
    tree: tree, viewport: viewport(1000, 900), sizing: PaneLayoutFixture.sizing)

  #expect(board.neighbor(of: "left", toward: .right, shown: nil) != nil)
  #expect(board.neighbor(of: "top", toward: .down, shown: nil) == "bottom")
  #expect(board.neighbor(of: "bottom", toward: .left, shown: nil) == "left")
  #expect(board.neighbor(of: "top", toward: .left, shown: nil) == "left")
}

@Test func theShownTabFollowsFocusThenMemoryThenTheFirstTab() throws {
  let board = try boardOf([.terminal, .agent], in: viewport(500, 400))

  #expect(board.shownTab(focused: "agent-1", remembered: "terminal-0") == "agent-1")
  #expect(board.shownTab(focused: "gone", remembered: "agent-1") == "agent-1")
  #expect(board.shownTab(focused: nil, remembered: "gone") == "terminal-0")
  let wide = try boardOf([.web, .terminal], in: viewport(800, 700))
  #expect(wide.shownTab(focused: "terminal-1", remembered: nil) == nil)
}

@Test func anArrowEntersATabGroupAtTheTabShown() throws {
  let tree = try PaneLayoutFixture.workspace([.editor, .terminal, .agent])
  let size = viewport(900, 400)
  let board = try PaneBoardPolicy.board(
    tree: tree, viewport: size, sizing: PaneLayoutFixture.sizing)

  let tabs = try #require(board.tabs)
  let beside = try #require(board.placements.first)
  #expect(tabs.paneIds.count == 2)
  #expect(
    board.neighbor(of: beside.paneId, toward: .right, shown: tabs.paneIds[1]) == tabs.paneIds[1])
  #expect(board.neighbor(of: beside.paneId, toward: .right, shown: nil) == tabs.paneIds[0])
  #expect(board.neighbor(of: tabs.paneIds[1], toward: .left, shown: nil) == beside.paneId)
}

// MARK: The client

private func pane(_ id: String, mode: PaneMode) -> Pane {
  PaneLayoutFixture.pane(id, workspace: "work", mode: mode)
}

/// The board of workspace `id` in an 800 by 700 viewport.
@MainActor private func workspaceBoard(of client: EngineClient, workspace id: String = "work")
  -> PaneBoard?
{
  client.paneBoard(
    workspaceId: id, viewport: viewport(800, 700), sizing: PaneLayoutFixture.sizing)
}

@MainActor private func client(
  panes: [Pane], layout: PaneLayoutState? = nil
) throws -> EngineClient {
  let client = EngineClient(socket: "/tmp/pane-board.sock")
  for (index, pane) in panes.enumerated() {
    try client.applyPaneCreated(
      PaneCreatedEvent(eventId: String(index + 1), at: "2026-10-19T12:00:00Z", pane: pane))
  }
  let encoded: JsonValue = try layout.map { try JsonValue.encoded($0.envelope()) } ?? .object([:])
  let workspace = Workspace(workspaceId: "work", name: "work", path: "/repo", layout: encoded)
  client.upsertWorkspace(workspace)
  client.paneLayouts.synchronize(
    workspaceId: "work", confirmed: client.paneLayoutState(for: workspace))
  return client
}

@MainActor @Test func theBoardFollowsTheLayoutTheEngineStoresForTheWorkspace() throws {
  // The Mac arranged the agent to the left of the terminal.
  let arranged = PaneLayoutState(
    tree: .split(
      PaneLayoutSplit(
        axis: .horizontal,
        children: [
          PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "agent", kind: .agent))),
          PaneLayoutBranch(tree: .leaf(PaneLayoutLeaf(paneId: "shell", kind: .terminal))),
        ])),
    mode: .custom)
  let client = try client(
    panes: [pane("shell", mode: .terminal), pane("agent", mode: .agent)], layout: arranged)

  let board = try #require(workspaceBoard(of: client))

  #expect(board.paneIds == ["agent", "shell"])
}

@MainActor @Test func aPaneTheMacHasNotArrangedYetJoinsTheBoard() throws {
  let client = try client(panes: [pane("shell", mode: .terminal), pane("agent", mode: .agent)])

  let board = try #require(workspaceBoard(of: client))

  #expect(Set(board.paneIds) == ["shell", "agent"])
  #expect(workspaceBoard(of: client, workspace: "other") == nil)
}

@MainActor @Test func aClosedPaneLeavesTheBoardBeforeTheLayoutCatchesUp() throws {
  let arranged = PaneLayoutState(
    tree: try PaneLayoutFixture.workspace([.terminal, .agent]), mode: .custom)
  let client = try client(
    panes: [pane("terminal-0", mode: .terminal), pane("agent-1", mode: .agent)], layout: arranged)

  try client.applyPaneEvent(.paneClosed, .object(["paneId": .string("agent-1")]))
  let board = try #require(workspaceBoard(of: client))

  #expect(board.paneIds == ["terminal-0"])
}

@Test func theBoardNeedsAWideLayoutAndMoreThanOnePane() {
  let two = [pane("a", mode: .terminal), pane("b", mode: .agent)]

  #expect(PaneBoardPolicy.usesBoard(regularWidth: true, panes: two))
  #expect(!PaneBoardPolicy.usesBoard(regularWidth: true, panes: [two[0]]))
  #expect(!PaneBoardPolicy.usesBoard(regularWidth: true, panes: []))
  #expect(!PaneBoardPolicy.usesBoard(regularWidth: false, panes: two + [pane("c", mode: .web)]))
}

@Test func aWorkspaceWithAWritingPaneKeepsTheWritingWorkspace() {
  let panes = [pane("a", mode: .terminal), pane("doc", mode: .writing)]

  #expect(!PaneBoardPolicy.usesBoard(regularWidth: true, panes: panes))
}

@MainActor @Test func everyAgentOfTheBoardHydratesAtOnce() throws {
  let client = try client(panes: [
    pane("shell", mode: .terminal), pane("one", mode: .agent), pane("two", mode: .agent),
  ])

  let presented = client.presentedAgents(in: ["two", "shell", "one", "gone"])

  #expect(presented.map(\.paneId) == ["two", "one"])
  #expect(presented.map(\.isVisible) == [true, true])
}

private func terminalPane(_ id: String, cols: UInt32, rows: UInt32) -> Pane {
  var pane = PaneLayoutFixture.pane(id, workspace: "work", mode: .terminal)
  pane.cols = cols
  pane.rows = rows
  return pane
}

@MainActor @Test func eachBoardTerminalObeysItsPanesSizeMode() throws {
  let client = try client(panes: [
    pane("following", mode: .terminal), pane("fitted", mode: .terminal),
  ])
  var resizes: [(String?, Int, Int)] = []
  func host(_ id: String, _ mode: TerminalSizeMode) -> TerminalHost {
    TerminalHost(
      client: client, paneId: id, acceptsInput: true, sizeMode: mode,
      resizeTerminal: { resizes.append(($0, $1, $2)) })
  }
  let following = host("following", .followMac)
  let fitted = host("fitted", .fitThisScreen)
  let followingView = TerminalView(frame: .zero)
  let fittedView = TerminalView(frame: .zero)

  // The board mounts both: the Mac's terminal is shown as it is and only the fitted one asks.
  following.update(
    followingView, pane: terminalPane("following", cols: 200, rows: 50), connectionRevision: 1)
  fitted.update(
    fittedView, pane: terminalPane("fitted", cols: 200, rows: 50), connectionRevision: 1)
  #expect(followingView.getTerminal().cols == 200)
  #expect(resizes.map(\.0) == ["fitted"])

  // The board lays out again (the window rotates, the panes stack): a cell of another size is
  // reported by the fitted terminal only.
  following.sizeChanged(source: followingView, newCols: 60, newRows: 20)
  fitted.sizeChanged(source: fittedView, newCols: 60, newRows: 20)
  #expect(resizes.map(\.0) == ["fitted", "fitted"])
  #expect(resizes.map(\.1) == [fittedView.getTerminal().cols, 60])
  // The following terminal went back to the size the client holds for its pane (the Mac's).
  #expect(followingView.getTerminal().cols == 80)
}
