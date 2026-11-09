import Foundation
import SoyehtProtocol
import Testing

@testable import SoyehtClient

private let sizing = PaneLayoutFixture.sizing

private func pane(_ id: String, mode: PaneMode = .terminal, cwd: String = "/repo") -> Pane {
  var pane = PaneLayoutFixture.pane(id, workspace: "w1", mode: mode)
  pane.cwd = cwd
  return pane
}

@Test func aSplitOpensWhatTheSourcePaneIs() {
  #expect(PaneSplitPolicy.kind(splitting: pane("a")) == .terminal)
  #expect(PaneSplitPolicy.kind(splitting: pane("a", mode: .editor)) == .terminal)
  #expect(PaneSplitPolicy.kind(splitting: pane("a", mode: .web)) == .terminal)
  #expect(PaneSplitPolicy.kind(splitting: pane("a", mode: .writing)) == .chooser)
  var agent = pane("a", mode: .agent)
  #expect(PaneSplitPolicy.kind(splitting: agent) == .terminal)  // no session yet
  agent.agent = AgentSession(
    harness: "claude", version: "1", fingerprint: "f", options: [], authMethods: [], queue: [],
    draft: false, role: .worker)
  #expect(PaneSplitPolicy.kind(splitting: agent) == .agent(harness: "claude"))
}

@Test func theNewPaneStartsInTheSourcePanesFolderElseTheWorkspaces() {
  #expect(
    PaneSplitPolicy.directory(of: pane("a", cwd: "/repo/sub"), workspacePath: "/repo")
      == "/repo/sub")
  #expect(PaneSplitPolicy.directory(of: pane("a", cwd: ""), workspacePath: "/repo") == "/repo")
  #expect(PaneSplitPolicy.directory(of: pane("a", cwd: ""), workspacePath: "") == nil)
  #expect(PaneSplitPolicy.directory(of: pane("a", cwd: ""), workspacePath: nil) == nil)
}

/// A client whose engine is a stub that makes panes `n1`, `n2`, ... and stores the layout.
@MainActor private func splitClient(
  workspacePath: String, panes: [Pane], seen: Seen
) -> EngineClient {
  let client = EngineClient(socket: "/tmp/pane-split.sock")
  let workspace = Workspace(
    workspaceId: "w1", name: "loadtest", path: workspacePath, layout: .object([:]))
  client.requestStub = { method, params in
    seen.calls.append((method, params))
    switch method {
    case .paneCreate:
      seen.created += 1
      let id = "n\(seen.created)"
      client.panes.append(pane(id))
      return .object(["paneId": .string(id)])
    case .workspaceLayoutSet:
      seen.layoutSets += 1
      var stored = workspace
      stored.layout = params["layout"]
      return try .object(["workspace": .encoded(stored)])
    default: throw ClientFailure.invalidFrame
    }
  }
  client.upsertWorkspace(workspace)
  client.panes = panes
  client.paneLayouts.synchronize(
    workspaceId: "w1", confirmed: client.paneLayoutState(for: workspace))
  return client
}

@MainActor private final class Seen {
  var layoutSets = 0
  var calls: [(RequestMethod, JsonValue)] = []
  var created = 0
  func requests(_ method: RequestMethod) -> [JsonValue] {
    calls.filter { $0.0 == method }.map(\.1)
  }
}

/// Waits for the layout the split stores, which the controller persists in the background.
@MainActor private func storedLayout(_ seen: Seen) async -> JsonValue? {
  for _ in 0..<200 {
    if seen.layoutSets > 0, let layout = seen.requests(.workspaceLayoutSet).last { return layout }
    try? await Task.sleep(for: .milliseconds(10))
  }
  return nil
}

@MainActor @Test func aSplitMakesTheShellInTheSameWorkspaceAndFolderWhateverTheSpelling()
  async throws
{
  // Foundation reads `/private/tmp/x` as `/tmp/x`; the workspace is stored as it was made.
  let seen = Seen()
  let client = splitClient(
    workspacePath: "/private/tmp/loadtest", panes: [pane("a", cwd: "/private/tmp/loadtest")],
    seen: seen)

  let created = try await client.split("a", axis: .horizontal, sizing: sizing)

  #expect(created == "n1")
  #expect(seen.requests(.workspaceCreate).isEmpty)
  let request = try #require(seen.requests(.paneCreate).first)
  #expect(request["workspaceId"].string == "w1")
  #expect(request["mode"].string == PaneMode.terminal.rawValue)
  #expect(request["cwd"].string == "/private/tmp/loadtest")
  #expect(client.selectedPaneId == "n1")
  let tree = try #require(await storedTree(seen, client))
  #expect(tree.paneIdsInCanonicalOrder == ["a", "n1"])
}

@MainActor @Test func splittingAnAgentOpensAnAgentOfTheSameHarnessInTheSameWorkspace() async throws
{
  let seen = Seen()
  var agent = pane("a", mode: .agent, cwd: "/private/tmp/loadtest")
  agent.agent = AgentSession(
    harness: "claude", version: "1", fingerprint: "f", options: [], authMethods: [], queue: [],
    draft: false, role: .worker)
  let client = splitClient(workspacePath: "/private/tmp/loadtest", panes: [agent], seen: seen)

  _ = try await client.split("a", axis: .vertical, sizing: sizing)

  #expect(seen.requests(.workspaceCreate).isEmpty)
  let request = try #require(seen.requests(.paneCreate).first)
  #expect(request["workspaceId"].string == "w1")
  #expect(request["mode"].string == PaneMode.agent.rawValue)
  #expect(request["harness"].string == "claude")
  #expect(request["cwd"].string == "/private/tmp/loadtest")
}

@MainActor @Test func aWritingPaneIsNotSplitWithoutTheChooser() async throws {
  let seen = Seen()
  let client = splitClient(workspacePath: "/repo", panes: [pane("a", mode: .writing)], seen: seen)
  await #expect(throws: ClientFailure.self) {
    try await client.split("a", axis: .horizontal, sizing: sizing)
  }
  #expect(seen.requests(.paneCreate).isEmpty)
}

/// The tree six splits make: right, down, right, down, right, each of the shell the last one made.
@MainActor private func sixShells() async throws -> PaneLayoutTree {
  let seen = Seen()
  let client = splitClient(workspacePath: "/repo", panes: [pane("a")], seen: seen)
  var anchor = "a"
  for axis: PaneLayoutAxis in [.horizontal, .vertical, .horizontal, .vertical, .horizontal] {
    seen.layoutSets = 0
    anchor = try await client.split(anchor, axis: axis, sizing: sizing)
    _ = await storedLayout(seen)
  }
  #expect(seen.requests(.workspaceCreate).isEmpty)
  #expect(seen.requests(.paneCreate).allSatisfy { $0["workspaceId"].string == "w1" })
  return try #require(await storedTree(seen, client))
}

/// Every pane the board draws keeps its floor; whatever does not fit is folded into tabs.
private func expectFloors(_ board: PaneBoard) throws {
  let floor = try #require(sizing.floors[.terminal])
  for placement in board.placements {
    #expect(placement.rect.size.width >= floor.width, "\(placement.paneId)")
    #expect(placement.rect.size.height >= floor.height, "\(placement.paneId)")
  }
}

@MainActor @Test func splittingAgainAndAgainNestsSixShellsThatAllFitAWideWindow() async throws {
  let tree = try await sixShells()
  #expect(Set(tree.paneIdsInCanonicalOrder) == ["a", "n1", "n2", "n3", "n4", "n5"])
  let board = try PaneBoardPolicy.board(
    tree: tree, viewport: PaneLayoutSize(width: 2400, height: 1400), sizing: sizing)
  #expect(board.tabs == nil)
  #expect(board.placements.count == 6)
  try expectFloors(board)
}

@MainActor @Test func aWindowTooSmallForSixShellsFoldsThemIntoTabsInsteadOfBreakingTheirFloors()
  async throws
{
  let board = try PaneBoardPolicy.board(
    tree: try await sixShells(), viewport: PaneLayoutSize(width: 700, height: 500),
    sizing: sizing)
  #expect(board.paneIds.count == 6)
  try expectFloors(board)
}

@MainActor @Test func closingAShellCollapsesItsSplit() throws {
  var tree = PaneLayoutTree.leaf(PaneLayoutLeaf(paneId: "a", kind: .terminal))
  for (id, axis) in [("b", PaneLayoutAxis.horizontal), ("c", .vertical)] {
    tree = try PaneLayoutTreeMutation.splitting(
      tree, anchorPaneId: id == "b" ? "a" : "b",
      newLeaf: PaneLayoutLeaf(paneId: id, kind: .terminal),
      axis: axis)
  }
  let closed = try #require(tree.removingPane(withId: "c"))
  #expect(closed.paneIdsInCanonicalOrder == ["a", "b"])
  guard case .split(let split) = closed else {
    Issue.record("the pair stays a split")
    return
  }
  #expect(split.children.count == 2)
  guard case .leaf = split.children[1].tree else {
    Issue.record("b is a leaf again")
    return
  }
}

/// The tree of the last layout the split stored.
@MainActor private func storedTree(_ seen: Seen, _ client: EngineClient) async -> PaneLayoutTree? {
  guard let layout = await storedLayout(seen) else { return nil }
  return PaneLayoutState(
    workspaceLayout: layout["layout"], livePanes: client.layoutPanes(client.panes)
  ).tree
}
