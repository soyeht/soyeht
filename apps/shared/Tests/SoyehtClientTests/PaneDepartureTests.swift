import SoyehtProtocol
import Testing

@testable import SoyehtClient

private func agentPane(_ id: String, workspace: String = "work") -> Pane {
  PaneLayoutFixture.pane(id, workspace: workspace, mode: .agent)
}

@MainActor private func client(holding panes: [Pane]) throws -> EngineClient {
  let client = EngineClient(socket: "/tmp/pane-departure.sock")
  for (index, pane) in panes.enumerated() {
    try client.applyPaneCreated(
      PaneCreatedEvent(eventId: String(index + 1), at: "2026-10-18T12:00:00Z", pane: pane))
  }
  return client
}

private let notFound = ClientFailure.remote(ErrorKind.not_found.rawValue, "Pane not found")

@MainActor @Test func aLateUpdateAfterThePaneClosedDoesNotBringItBack() throws {
  let client = try client(holding: [agentPane("a"), agentPane("b")])

  try client.applyPaneEvent(.paneClosed, .object(["paneId": .string("b")]))
  var late = agentPane("b")
  late.state = .dead
  try client.applyPaneEvent(.paneUpdated, .object(["pane": .encoded(late)]))
  try client.applyPaneEvent(.paneRenamed, .object(["pane": .encoded(late)]))

  #expect(client.panes.map(\.paneId) == ["a"])
}

@MainActor @Test func closingTheSelectedPaneSelectsAnotherPaneOfItsWorkspaceFirst() throws {
  let client = try client(holding: [
    agentPane("other", workspace: "elsewhere"), agentPane("a"), agentPane("b"),
  ])
  client.select("b")

  try client.applyPaneEvent(.paneClosed, .object(["paneId": .string("b")]))

  #expect(client.selectedPaneId == "a")
}

@MainActor @Test func aPaneTheEngineDoesNotHoldDropsOutWhenFocusIsRefused() async throws {
  let client = try client(holding: [agentPane("a"), agentPane("gone")])
  client.requestStub = { method, _ in
    if method == .paneFocus { throw notFound }
    throw ClientFailure.invalidFrame
  }

  let focused = try await client.focusEnginePane("gone")

  #expect(!focused)
  #expect(client.panes.map(\.paneId) == ["a"])
  #expect(client.problem == nil)
}

@MainActor @Test func aPaneTheEngineDoesNotHoldDropsOutWhenItsCommandsAreRefused() async throws {
  let client = try client(holding: [agentPane("a"), agentPane("gone")])
  client.requestStub = { method, params in
    switch method {
    case .skillsList: return try .encoded(SkillsListResult(items: [], userDirectory: "/skills"))
    case .paneCommandsList where params["paneId"].string == "gone": throw notFound
    default: throw ClientFailure.invalidFrame
    }
  }

  await client.loadSlashSources(paneId: "gone")

  #expect(client.panes.map(\.paneId) == ["a"])
  #expect(client.problem == nil)
}

@MainActor @Test func aFailureThatIsNotAMissingPaneIsStillShown() async throws {
  let client = try client(holding: [agentPane("a")])
  client.requestStub = { _, _ in throw ClientFailure.remote("internal_error", "Engine failed") }

  let focused = try? await client.focusEnginePane("a")
  await client.loadSlashSources(paneId: "a")

  #expect(focused == nil)
  #expect(client.panes.map(\.paneId) == ["a"])
  #expect(client.problem == "Engine failed")
}

@MainActor private func settle(until done: () -> Bool) async {
  for _ in 0..<200 where !done() { await Task.yield() }
}

@MainActor @Test func everyWayToStoreAPaneRefusesOneThatClosed() throws {
  let client = try client(holding: [agentPane("a"), agentPane("b")])
  try client.applyPaneEvent(.paneClosed, .object(["paneId": .string("b")]))

  #expect(!client.upsertPane(agentPane("b")))
  #expect(client.upsertPane(agentPane("c")))
  try client.applyPaneCreated(
    PaneCreatedEvent(eventId: "9", at: "2026-10-18T12:00:00Z", pane: agentPane("b")))

  #expect(client.panes.map(\.paneId) == ["a", "c"])
}

@MainActor private func dropsQuietly(
  _ start: (EngineClient) -> Void
) async throws {
  let client = try client(holding: [agentPane("a"), agentPane("gone")])
  client.requestStub = { _, _ in throw notFound }

  start(client)
  await settle { client.panes.count == 1 }

  #expect(client.panes.map(\.paneId) == ["a"])
  #expect(client.problem == nil)
}

@MainActor @Test func aMissingPaneDropsQuietlyWhenItsQueueRefreshIsRefused() async throws {
  try await dropsQuietly { $0.refreshAgentQueue("gone") }
}

@MainActor @Test func aMissingPaneDropsQuietlyWhenItsTranscriptRefreshIsRefused() async throws {
  try await dropsQuietly { $0.refreshAgent("gone") }
}

@MainActor @Test func aMissingPaneDropsQuietlyWhenOlderMessagesAreRefused() async throws {
  try await dropsQuietly { client in
    var view = AgentPaneView()
    view.nextTranscriptCursor = "older"
    client.agentPanes["gone"] = view
    client.loadOlderAgentMessages("gone")
  }
}

@MainActor @Test func aBackgroundPaneRequestThatFailsOtherwiseIsStillShown() async throws {
  let client = try client(holding: [agentPane("a")])
  client.requestStub = { _, _ in throw ClientFailure.remote("internal_error", "Engine failed") }

  client.refreshAgentQueue("a")
  await settle { client.problem != nil }

  #expect(client.problem == "Engine failed")
  #expect(client.panes.map(\.paneId) == ["a"])
}

@MainActor @Test func aWorkspaceTheEngineDoesNotHoldKeepsItsPaneAndRaisesNothing() async throws {
  let client = try client(holding: [agentPane("a")])
  client.requestStub = { method, _ in
    if method == .skillsList {
      throw ClientFailure.remote(ErrorKind.not_found.rawValue, "Workspace not found")
    }
    throw ClientFailure.invalidFrame
  }

  await client.loadSlashSources(paneId: "a")

  #expect(client.panes.map(\.paneId) == ["a"])
  #expect(client.problem == nil)
}
